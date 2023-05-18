pub fn as_hex(vec: &[u8]) -> String {
    vec.iter()
        .flat_map(|&x| [inner_hex(x >> 4), inner_hex(x & 0x0F)])
        .collect()
}

fn inner_hex(x: u8) -> char {
    match x {
        0 => '0',
        1 => '1',
        2 => '2',
        3 => '3',
        4 => '4',
        5 => '5',
        6 => '6',
        7 => '7',
        8 => '8',
        9 => '9',
        10 => 'A',
        11 => 'B',
        12 => 'C',
        13 => 'D',
        14 => 'E',
        15 => 'F',
        _ => '?',
    }
}
