/**
 * Resolve saved focus lazily after a Formula replaced its KaTeX markup. No renderer dependency.
 * A Formula with a phone layout holds two markups (`data-variant` wide/narrow, one shown by CSS); the saved
 * occurrence is looked up again inside the same variant, so focus never moves into the layout that is not shown.
 */
export function captureFormulaFocus(node:HTMLElement|null):()=>HTMLElement|null {
 const root=node?.closest<HTMLElement>('.lab-formula');
 const variant=node?.closest<HTMLElement>('[data-variant]')?.dataset.variant;
 const scope=()=>variant?root?.querySelector<HTMLElement>(`[data-variant="${variant}"]`)??null:root;
 const kind=node?.dataset.s?'s':'t',key=node?.dataset[kind];
 const candidates=()=>{const el=scope();return el?[...el.querySelectorAll<HTMLElement>('[data-'+kind+']')].filter(el=>
  el.dataset[kind]===key&&!el.closest('.is-hidden,[aria-hidden="true"],[inert]')):[]};
 const index=node?candidates().indexOf(node):-1;
 return ()=>{
  if(node?.isConnected&&!node.closest('.is-hidden,[aria-hidden="true"],[inert]'))return node;
  if(!root?.isConnected||!key||index<0)return null;
  return candidates()[index]??null;
 };
}
