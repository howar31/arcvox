export interface ToastItem { id: number; kind: "ok" | "error"; message: string }

const OK_MS = 2500;

/** Transient notices: confirmations fade on their own, errors stay until dismissed. */
class ToastStore {
  items = $state<ToastItem[]>([]);
  #next = 1;

  ok(message: string) {
    const id = this.#push("ok", message);
    setTimeout(() => this.dismiss(id), OK_MS);
  }

  error(message: string) { this.#push("error", message); }

  dismiss(id: number) { this.items = this.items.filter((t) => t.id !== id); }

  #push(kind: ToastItem["kind"], message: string): number {
    const id = this.#next++;
    this.items = [...this.items, { id, kind, message }];
    return id;
  }
}

export const toast = new ToastStore();
