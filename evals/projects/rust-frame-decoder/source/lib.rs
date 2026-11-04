mod length;
pub fn decode(input:&[u8])->Result<(Vec<Vec<u8>>,Vec<u8>),String>{
    let mut frames=Vec::new(); let mut offset=0;
    while input.len()-offset>=2 {
        let len=length::read(&input[offset..]) as usize;
        if len>32{return Err("limit".into());}
        offset+=2;
        if input.len()-offset<len {break;}
        frames.push(input[offset..offset+len].to_vec());offset+=len;
    }
    Ok((frames,Vec::new()))
}
