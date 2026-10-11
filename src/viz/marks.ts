export interface TermMark { hidden?: boolean; struck?: boolean; dim?: boolean }
export type Marks = Readonly<Record<string, TermMark>>;
export const markClass = (mark?: TermMark): string => (mark?.hidden ? ' is-hidden' : '') + (mark?.struck ? ' is-struck' : '') + (mark?.dim ? ' is-dim' : '');
