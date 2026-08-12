/** Temple-gesture focus ring for RayNeo `[data-rayneo-focus]` targets. */

export type TempleAction = 'forward' | 'backward' | 'up' | 'down' | 'click' | 'doubleclick';

export type TempleHandler = (action: TempleAction) => void;

const FOCUS_ATTR = 'data-rayneo-focus';
const FOCUS_CLASS = 'rayneo-focus';

declare global {
  interface Window {
    __bitfunTemple?: TempleHandler;
  }
}

function listFocusables(): HTMLElement[] {
  return Array.from(document.querySelectorAll<HTMLElement>(`[${FOCUS_ATTR}]`)).filter(
    (el) => {
      if (el.closest('[hidden], [aria-hidden="true"]')) return false;
      const style = window.getComputedStyle(el);
      if (style.display === 'none' || style.visibility === 'hidden') return false;
      if (el.getAttribute('disabled') != null) return false;
      if ((el as HTMLButtonElement).disabled) return false;
      return true;
    },
  );
}

function clearFocusClass(): void {
  document.querySelectorAll(`.${FOCUS_CLASS}`).forEach((el) => {
    el.classList.remove(FOCUS_CLASS);
  });
}

export function focusAt(index: number): number {
  const items = listFocusables();
  if (items.length === 0) {
    clearFocusClass();
    return -1;
  }
  const next = ((index % items.length) + items.length) % items.length;
  clearFocusClass();
  items[next].classList.add(FOCUS_CLASS);
  items[next].scrollIntoView({ block: 'nearest', inline: 'nearest' });
  return next;
}

export function currentFocusIndex(): number {
  const items = listFocusables();
  const active = document.querySelector<HTMLElement>(`.${FOCUS_CLASS}[${FOCUS_ATTR}]`);
  if (!active) return -1;
  return items.indexOf(active);
}

function activateFocused(): void {
  const items = listFocusables();
  const idx = currentFocusIndex();
  const target = idx >= 0 ? items[idx] : items[0];
  if (!target) return;
  if (idx < 0) {
    focusAt(0);
  }
  target.click();
}

export type TempleFocusOptions = {
  /** Called after focus index changes (optional host analytics / sync). */
  onFocusIndexChange?: (index: number) => void;
  /** Right-temple double-click (e.g. chat → session list). */
  onDoubleClick?: () => void;
  /**
   * Vertical temple slide. Return true when handled (e.g. content scroll)
   * so focus does not move. `direction` is -1 for up, +1 for down.
   */
  onVerticalSlide?: (direction: -1 | 1) => boolean;
  /**
   * Single temple click. Return true when handled (e.g. start voice)
   * so the focused control is not activated.
   */
  onClick?: () => boolean;
};

/** Install temple focus handlers for the glasses SPA. */
export function installTempleFocus(options: TempleFocusOptions = {}): () => void {
  let index = currentFocusIndex();
  const previous = window.__bitfunTemple;

  const emit = (next: number) => {
    index = next;
    options.onFocusIndexChange?.(next);
  };

  const handler: TempleHandler = (action) => {
    switch (action) {
      case 'forward':
        emit(focusAt(currentFocusIndex() + 1));
        break;
      case 'backward':
        emit(focusAt(currentFocusIndex() <= 0 ? listFocusables().length - 1 : currentFocusIndex() - 1));
        break;
      case 'up':
        if (options.onVerticalSlide?.(-1)) break;
        emit(focusAt(currentFocusIndex() <= 0 ? listFocusables().length - 1 : currentFocusIndex() - 1));
        break;
      case 'down':
        if (options.onVerticalSlide?.(1)) break;
        emit(focusAt(currentFocusIndex() + 1));
        break;
      case 'click':
        if (options.onClick?.()) break;
        activateFocused();
        break;
      case 'doubleclick':
        options.onDoubleClick?.();
        break;
      default:
        break;
    }
  };

  window.__bitfunTemple = handler;

  if (index < 0) {
    emit(focusAt(0));
  }

  const mo = new MutationObserver(() => {
    const items = listFocusables();
    if (items.length === 0) {
      clearFocusClass();
      emit(-1);
      return;
    }
    if (currentFocusIndex() < 0) {
      emit(focusAt(Math.min(Math.max(index, 0), items.length - 1)));
    }
  });
  mo.observe(document.body, {
    childList: true,
    subtree: true,
    attributes: true,
    attributeFilter: ['hidden', 'aria-hidden', 'disabled', FOCUS_ATTR],
  });

  return () => {
    mo.disconnect();
    clearFocusClass();
    if (window.__bitfunTemple === handler) {
      if (previous) window.__bitfunTemple = previous;
      else delete window.__bitfunTemple;
    }
  };
}
