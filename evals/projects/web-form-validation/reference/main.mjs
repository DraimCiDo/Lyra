export function quantity(value){if(typeof value!=='string'||!/^\d+$/.test(value.trim()))return null;const n=Number(value.trim());return Number.isSafeInteger(n)&&n>=1&&n<=999?n:null;}
