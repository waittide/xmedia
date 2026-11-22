export function stripAriaModalOnLeave(el: Element): void {
  el.removeAttribute('aria-modal')
  el.querySelectorAll('[aria-modal]').forEach((node) => node.removeAttribute('aria-modal'))
}
