import {matches} from './filter.mjs';
export function select(rows,query,activeOnly){return rows.sort((a,b)=>b.score-a.score).filter(row=>matches(row,query));}
