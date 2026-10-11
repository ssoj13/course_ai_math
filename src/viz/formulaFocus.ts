/** Resolve saved focus lazily after a Formula replaced its KaTeX markup. No renderer dependency. */
export function captureFormulaFocus(node:HTMLElement|null):()=>HTMLElement|null {
 const root=node?.closest<HTMLElement>('.lab-formula');
 const kind=node?.dataset.s?'s':'t',key=node?.dataset[kind];
 const candidates=()=>root?[...root.querySelectorAll<HTMLElement>('[data-'+kind+']')].filter(el=>
  el.dataset[kind]===key&&!el.closest('.lab-formula-reserve,.is-hidden,[aria-hidden="true"],[inert]')):[];
 const index=node?candidates().indexOf(node):-1;
 return ()=>{
  if(node?.isConnected&&!node.closest('.lab-formula-reserve,.is-hidden,[aria-hidden="true"],[inert]'))return node;
  if(!root?.isConnected||!key||index<0)return null;
  return candidates()[index]??null;
 };
}
