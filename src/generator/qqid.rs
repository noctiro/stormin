use rand::RngCore;

pub fn generate_qq_id<T: RngCore>(rng: &mut T) -> String {
    // 生成长度：6 到 12
    let length = (rng.next_u32() % 7) as usize + 6;
    
    // 准备存储结果
    let mut result = String::with_capacity(length);
    
    // 生成第一位（1-9）
    let rand_val = rng.next_u32();
    result.push(char::from_digit(rand_val % 9 + 1, 10).unwrap());
    
    // 一次性生成所有剩余位数
    // 每个u32提供约9位十进制数字的随机性
    let remaining = length - 1;
    let mut i = 0;
    
    while i < remaining {
        let mut n = rng.next_u32();
        let mut digits_to_add = std::cmp::min(9, remaining - i);
        
        while digits_to_add > 0 {
            result.push(char::from_digit(n % 10, 10).unwrap());
            n /= 10;
            digits_to_add -= 1;
            i += 1;
        }
    }
    
    result
}