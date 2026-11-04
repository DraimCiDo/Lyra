import {matches} from './filter.mjs';
export function select(rows,query,activeOnly){return rows.filter(row=>(!activeOnly||row.active===true)&&matches(row,query)).toSorted((a,b)=>a.score-b.score);}
