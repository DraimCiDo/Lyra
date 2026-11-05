pub fn merge(input:&[(u32,u32)])->Result<Vec<(u32,u32)>,String>{
    if input.iter().any(|(a,b)|a>b){return Err("bounds".into());}
    let mut sorted:Vec<_>=input.iter().copied().filter(|(a,b)|a<b).collect();sorted.sort_unstable();
    let mut out:Vec<(u32,u32)>=Vec::new();
    for (a,b) in sorted {if let Some(last)=out.last_mut(){if a<=last.1 {last.1=last.1.max(b);continue;}}out.push((a,b));}Ok(out)
}
