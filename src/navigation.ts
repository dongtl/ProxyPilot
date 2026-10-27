import { ref } from "vue";

export type PageKey = "home" | "profiles" | "connections" | "settings";

export const currentPage = ref<PageKey>("home");

export function goTo(page: PageKey): void {
  currentPage.value = page;
}
