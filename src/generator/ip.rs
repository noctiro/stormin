use rand::RngCore;
use std::fmt::Write;

pub fn generate_ipv4<T: RngCore>(rng: &mut T) -> String {
    let ip_int: u32 = rng.next_u32();

    let mut ip_string = String::with_capacity(15); // "xxx.xxx.xxx.xxx" 最多15个字符

    write!(
        &mut ip_string,
        "{}.{}.{}.{}",
        (ip_int >> 24) & 0xFF,
        (ip_int >> 16) & 0xFF,
        (ip_int >> 8) & 0xFF,
        ip_int & 0xFF
    )
    .unwrap();

    ip_string
}

pub fn generate_ipv6<T: RngCore>(rng: &mut T) -> String {
    // 生成一个随机的 u128 值
    let ip_int: u128 = rng.next_u64() as u128 | (rng.next_u64() as u128) << 64;

    let mut ip_string = String::with_capacity(39); // "xxxx:xxxx:xxxx:xxxx:xxxx:xxxx:xxxx:xxxx" 总共39个字符

    write!(
        &mut ip_string,
        "{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}",
        (ip_int >> 112) & 0xFFFF,
        (ip_int >> 96) & 0xFFFF,
        (ip_int >> 80) & 0xFFFF,
        (ip_int >> 64) & 0xFFFF,
        (ip_int >> 48) & 0xFFFF,
        (ip_int >> 32) & 0xFFFF,
        (ip_int >> 16) & 0xFFFF,
        ip_int & 0xFFFF
    )
    .unwrap();

    ip_string
}
