import {subtotal} from './money.mjs';
export function checkout(lines,percent){return subtotal(lines)*percent/100;}
