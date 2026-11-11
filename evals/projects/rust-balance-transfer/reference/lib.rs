mod policy;
use std::collections::BTreeMap;
pub fn transfer(db:&mut BTreeMap<String,i64>,from:&str,to:&str,amount:i64)->Result<(), &'static str>{
    if !policy::valid(amount){return Err("amount");}
    let source=*db.get(from).ok_or("missing")?;let target=*db.get(to).ok_or("missing")?;
    if source<amount{return Err("funds");}if from==to{return Ok(());}
    let credit=target.checked_add(amount).ok_or("overflow")?;
    db.insert(from.into(),source-amount);db.insert(to.into(),credit);Ok(())
}
