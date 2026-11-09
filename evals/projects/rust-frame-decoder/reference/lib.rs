mod length;
pub fn decode(input:&[u8])->Result<(Vec<Vec<u8>>,Vec<u8>),String>{
    let mut frames=Vec::new();let mut offset=0;
    while input.len()-offset>=2 {
        let len=length::read(&input[offset..]) as usize;
        if len>32{return Err("limit".into());}
        if input.len()-offset-2<len {break;}
        frames.push(input[offset+2..offset+2+len].to_vec());offset+=2+len;
    }
    Ok((frames,input[offset..].to_vec()))
}
