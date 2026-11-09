mod policy;
use std::collections::BTreeMap;
pub fn transfer(db:&mut BTreeMap<String,i64>,from:&str,to:&str,amount:i64)->Result<(), &'static str>{
    if !policy::valid(amount){return Err("amount");}
    *db.get_mut(to).ok_or("missing")?+=amount;
    let balance=db.get_mut(from).ok_or("missing")?;
    if *balance<amount{return Err("funds");}*balance-=amount;Ok(())
}
