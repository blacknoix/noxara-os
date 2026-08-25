import '@testing-library/jest-dom/vitest';

// axe-core probes canvas for ligature detection; jsdom lacks a real canvas.
HTMLCanvasElement.prototype.getContext = (() => ({
  measureText: () => ({ width: 0 }),
  fillText: () => undefined,
  clearRect: () => undefined,
})) as unknown as typeof HTMLCanvasElement.prototype.getContext;

// jsdom does not implement getComputedStyle(..., pseudoElt); axe contrast checks call it.
const originalGetComputedStyle = window.getComputedStyle.bind(window);
window.getComputedStyle = ((elt: Element, pseudoElt?: string | null) => {
  if (pseudoElt) {
    return {
      getPropertyValue: () => '',
      content: 'none',
    } as unknown as CSSStyleDeclaration;
  }
  return originalGetComputedStyle(elt);
}) as typeof window.getComputedStyle;
