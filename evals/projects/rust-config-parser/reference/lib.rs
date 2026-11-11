pub fn parse(text:&str)->Result<std::collections::BTreeMap<String,String>,String>{
    let mut out=std::collections::BTreeMap::new();
    for line in text.lines(){let line=line.trim();if line.is_empty()||line.starts_with('#'){continue;}
        let (key,value)=line.split_once('=').ok_or("missing equals")?;let key=key.trim();if key.is_empty()||out.contains_key(key){return Err("key".into());}out.insert(key.into(),value.trim().into());}
    Ok(out)
}
