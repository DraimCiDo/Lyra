export function matches(row,query){return row.name.toLocaleLowerCase('en').includes(query.trim().toLocaleLowerCase('en'));}
