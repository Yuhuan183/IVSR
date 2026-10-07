export type View = "process" | "browse" | "models";

class NavStore {
  view = $state<View>("process");
}

export const nav = new NavStore();
