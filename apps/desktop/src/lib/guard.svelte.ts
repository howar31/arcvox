/** Tracks unsaved changes on the current page so navigation can ask first. */
class Guard {
  dirty = $state(false);
  pending = $state(false);
  #action: (() => void) | null = null;

  set(dirty: boolean) { this.dirty = dirty; }

  /** Runs `action` now when nothing is unsaved; otherwise holds it for `confirm`. */
  request(action: () => void) {
    if (!this.dirty) { action(); return; }
    this.#action = action;
    this.pending = true;
  }

  /** Discards the unsaved changes and runs the held action. */
  confirm() {
    const action = this.#action;
    this.#action = null;
    this.pending = false;
    this.dirty = false;
    action?.();
  }

  cancel() {
    this.#action = null;
    this.pending = false;
  }
}

export const guard = new Guard();
