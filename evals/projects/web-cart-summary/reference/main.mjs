import {subtotal} from './money.mjs';
export function checkout(lines,percent){if(!Number.isFinite(percent)||percent<0||percent>100)throw new RangeError('discount');return Math.round(subtotal(lines)*(1-percent/100));}
