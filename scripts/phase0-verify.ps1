# Phase 0 实机验证：临时启动一次 sing-box 虚拟网卡，检查分流是否按设计工作，结束后自动停止。
#
# 必须在“以管理员身份运行”的 PowerShell 里执行。运行期间系统路由会经过 ProxyPilot 虚拟网卡，
# 脚本结束（包括出错或按 Ctrl+C）时会停止 sing-box，网卡随之消失。
#
# 用法示例：
#   powershell -ExecutionPolicy Bypass -File scripts\phase0-verify.ps1
#   powershell -ExecutionPolicy Bypass -File scripts\phase0-verify.ps1 -Dns system -IntranetUrl http://oa.corp.example.cn -IntranetHost oa.corp.example.cn
#   powershell -ExecutionPolicy Bypass -File scripts\phase0-verify.ps1 -HoldSeconds 120   # 保持 2 分钟，手动打开内网/应用试用
param(
    [string]$Upstream = '',            # 例如 socks5://127.0.0.1:10808；留空则自动探测
    [ValidateSet('encrypted', 'system')][string]$Dns = 'encrypted',
    [string]$IntranetUrl = '',         # 公司内网网页，检查能否打开
    [string]$IntranetHost = '',        # 公司内网域名，检查能否解析
    [string[]]$IntranetDomain = @(),   # 追加的内网域名后缀
    [int]$HoldSeconds = 0
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$cli = Join-Path $root 'target\debug\proxypilot-cli.exe'
$core = Join-Path $root 'src-tauri\binaries\sing-box-x86_64-pc-windows-msvc.exe'
$work = Join-Path $root 'runtime-phase0'
$report = New-Object System.Collections.Generic.List[string]
function Note($text) { Write-Host $text; $report.Add($text) }

$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw '请在“以管理员身份运行”的 PowerShell 里执行本脚本' }
if (-not (Test-Path $cli)) { throw "找不到 $cli，请先运行：cargo build -p proxypilot-core --bin proxypilot-cli" }
if (-not (Test-Path $core)) { throw '找不到 sing-box，请先运行 scripts\fetch-sing-box.ps1' }
if (Get-NetAdapter -Name 'ProxyPilot' -ErrorAction SilentlyContinue) { throw '已有名为 ProxyPilot 的网卡，请先退出正在运行的 ProxyPilot' }
New-Item -ItemType Directory -Force $work | Out-Null

if (-not $Upstream) {
    $found = & $cli detect | ConvertFrom-Json | Where-Object { $_.status -eq 'usable' } | Select-Object -First 1
    if (-not $found) { throw '没有探测到可用的 SOCKS5 端口，请打开代理客户端，或用 -Upstream 指定' }
    $Upstream = "socks5://$($found.host):$($found.port)"
    Note "自动选择上游：$($found.client) $Upstream"
}

Note "== 环境检查 =="
& $cli env --upstream $Upstream | ConvertFrom-Json | ForEach-Object { Note "[$($_.level)] $($_.title)：$($_.detail)" }

$config = Join-Path $work 'sing-box.json'
$log = Join-Path $work 'sing-box.log'
Remove-Item $log -ErrorAction SilentlyContinue
$genArgs = @('gen-config', '--upstream', $Upstream, '--out', $config, '--log', $log, '--dns', $Dns, '--log-level', 'info',
    '--preset', 'google', '--preset', 'github', '--preset', 'openai')
foreach ($d in $IntranetDomain) { $genArgs += @('--intranet', $d) }
$gen = & $cli @genArgs | ConvertFrom-Json
Note "DNS 模式：$Dns；上游占用进程：$($gen.upstreamOwner)"

$proc = $null
try {
    $proc = Start-Process -FilePath $core -ArgumentList @('run', '-c', "`"$config`"", '-D', "`"$work`"") -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput (Join-Path $work 'stdout.log') -RedirectStandardError (Join-Path $work 'stderr.log')
    $deadline = (Get-Date).AddSeconds(10)
    do { Start-Sleep -Milliseconds 300; $nic = Get-NetAdapter -Name 'ProxyPilot' -ErrorAction SilentlyContinue }
    until (($nic -and $nic.Status -eq 'Up') -or $proc.HasExited -or (Get-Date) -gt $deadline)
    if (-not $nic -or $nic.Status -ne 'Up') { throw "虚拟网卡没有就绪，见 $work\stderr.log" }
    Note "虚拟网卡已就绪"

    Note "== 连通性 =="
    $checks = @(
        @{ Name = '国内网站（应直连）'; Url = 'https://www.baidu.com/'; Host = 'www.baidu.com'; Expect = 'direct' },
        @{ Name = 'Google（应走代理）'; Url = 'https://www.google.com/generate_204'; Host = 'www.google.com'; Expect = 'proxy' },
        @{ Name = 'GitHub（应走代理）'; Url = 'https://github.com/'; Host = 'github.com'; Expect = 'proxy' }
    )
    if ($IntranetUrl) { $checks += @{ Name = '公司内网（应直连）'; Url = $IntranetUrl; Host = ([Uri]$IntranetUrl).Host; Expect = 'direct' } }
    foreach ($c in $checks) {
        $out = & curl.exe -s -o NUL -w '%{http_code} %{time_total}' --max-time 15 --noproxy '*' $c.Url 2>$null
        $c.Result = if ($LASTEXITCODE -eq 0) { "HTTP $($out.Split(' ')[0])，$($out.Split(' ')[1]) 秒" } else { "失败（curl 退出码 $LASTEXITCODE）" }
        Note ("{0}：{1}" -f $c.Name, $c.Result)
    }

    Note "== DNS =="
    foreach ($name in @('www.baidu.com', 'www.google.com') + @($IntranetHost | Where-Object { $_ })) {
        try {
            $a = (Resolve-DnsName -Name $name -Type A -DnsOnly -ErrorAction Stop | Where-Object Type -eq 'A' | Select-Object -First 2 -ExpandProperty IPAddress) -join ', '
            $aaaa = @(Resolve-DnsName -Name $name -Type AAAA -DnsOnly -ErrorAction SilentlyContinue | Where-Object Type -eq 'AAAA').Count
            Note "$name → A: $a；AAAA 记录数: $aaaa（ipv4_only 策略下应为 0）"
        } catch { Note "$name → 解析失败：$($_.Exception.Message)" }
    }
    $v6 = Get-NetIPAddress -AddressFamily IPv6 -ErrorAction SilentlyContinue | Where-Object { $_.PrefixOrigin -in 'RouterAdvertisement', 'Dhcp' -and $_.InterfaceAlias -ne 'ProxyPilot' }
    Note ("本机公网 IPv6：{0}" -f $(if ($v6) { '有' } else { '无（IPv6 相关项无法在这台机器上验证）' }))

    if ($HoldSeconds -gt 0) {
        Note "保持运行 $HoldSeconds 秒，可以手动打开内网、Codex、浏览器等试用……"
        Start-Sleep -Seconds $HoldSeconds
    }

    Start-Sleep -Seconds 1
    Note "== 实际走向（来自 sing-box 日志） =="
    $lines = Get-Content $log -ErrorAction SilentlyContinue
    foreach ($c in $checks) {
        $hit = $lines | Where-Object { $_ -match "outbound/[^\[]+\[(\w+)\]: outbound connection to $([regex]::Escape($c.Host)):" } | Select-Object -First 1
        $route = if ($hit -match '\[(\w+)\]: outbound connection') { $Matches[1] } else { '未出现在日志中' }
        $verdict = if ($route -eq $c.Expect) { '符合预期' } else { '不符合预期' }
        Note ("{0}：{1}，{2}" -f $c.Host, $route, $verdict)
    }
} finally {
    if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
    $gone = $false
    for ($i = 0; $i -lt 20 -and -not $gone; $i++) { Start-Sleep -Milliseconds 300; $gone = -not (Get-NetAdapter -Name 'ProxyPilot' -ErrorAction SilentlyContinue) }
    Note ("sing-box 已停止；虚拟网卡{0}" -f $(if ($gone) { '已移除' } else { '仍然存在（请记录下来，这是需要修的问题）' }))
    $report | Set-Content -Encoding utf8 (Join-Path $work 'report.txt')
    Write-Host "报告已保存到 $work\report.txt（不含节点信息，可以直接发给开发者）"
}
