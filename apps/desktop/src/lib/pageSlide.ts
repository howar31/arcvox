/**
 * The move between a sidebar entry's page and the pages under it: a page under the current one
 * slides in from the right over it, and on the way back the page slides out to the right over
 * the page above. A copy of the page that is leaving stands in for it while the new one renders.
 */
import { tick } from "svelte";

const DURATION = 240;
const EASING = "cubic-bezier(.22,.61,.36,1)";

let running: { shot: HTMLElement; done: () => void } | null = null;

/**
 * Runs `swap` (which shows the new route) inside the slide. `main` is the scrolling page area;
 * its grid cell in `shell` takes the copy.
 */
export function slide(shell: HTMLElement, main: HTMLElement, dir: 1 | -1, swap: () => void): void {
  running?.done();
  const page = main.querySelector<HTMLElement>(".page");
  if (!page || matchMedia("(prefers-reduced-motion: reduce)").matches) {
    swap();
    return;
  }
  const shot = document.createElement("div");
  shot.className = "slide-shot";
  shot.setAttribute("aria-hidden", "true");
  shot.inert = true;
  const copy = page.cloneNode(true) as HTMLElement;
  copy.style.transform = `translateY(${-main.scrollTop}px)`;
  shot.append(copy);
  shot.classList.add(dir > 0 ? "under" : "over");
  shell.append(shot);

  const moving = dir > 0 ? main : shot;
  const frames = dir > 0 ? [{ transform: "translateX(100%)" }, { transform: "translateX(0)" }] : [{ transform: "translateX(0)" }, { transform: "translateX(100%)" }];
  // The new page waits off to the right while it renders; the slide starts on the frame after,
  // so the time a page takes to render does not eat into the motion.
  if (dir > 0) {
    main.classList.add("sliding");
    main.style.transform = "translateX(100%)";
  }
  let anim: Animation | null = null;
  let frame = 0;
  const done = () => {
    cancelAnimationFrame(frame);
    anim?.cancel();
    shot.remove();
    main.classList.remove("sliding");
    main.style.transform = "";
    if (running?.shot === shot) running = null;
  };
  running = { shot, done };
  swap();
  tick().then(() => {
    if (running?.shot !== shot) return;
    frame = requestAnimationFrame(() => {
      main.style.transform = "";
      anim = moving.animate(frames, { duration: DURATION, easing: EASING });
      anim.finished.then(done, done);
    });
  });
}

let stopRestore: (() => void) | null = null;

/**
 * Scrolls `main` to `top`, again each time the page grows, until it gets there or the user
 * scrolls: a page coming back fills in what it loads after it is first shown.
 */
export function restoreScroll(main: HTMLElement, top: number): void {
  stopRestore?.();
  const page = main.querySelector<HTMLElement>(".page");
  const observer = new ResizeObserver(() => apply());
  const inputs = ["wheel", "pointerdown", "keydown", "touchstart"] as const;
  const stop = () => {
    observer.disconnect();
    for (const e of inputs) main.removeEventListener(e, stop);
    if (stopRestore === stop) stopRestore = null;
  };
  const apply = () => {
    main.scrollTop = top;
    if (Math.abs(main.scrollTop - top) < 1) stop();
  };
  stopRestore = stop;
  for (const e of inputs) main.addEventListener(e, stop, { passive: true });
  if (page) observer.observe(page);
  apply();
}

/** Stops a scroll that `restoreScroll` is still waiting to finish. */
export function cancelRestore(): void {
  stopRestore?.();
}
