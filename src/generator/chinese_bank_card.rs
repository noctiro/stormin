use rand::Rng;

// 中国主要银行的银行识别号(BIN)前缀 - 用u8数组而非字符串存储
static BANK_BINS: &[[u8; 6]] = &[
    [6, 2, 1, 2, 2, 6], // 工商银行
    [6, 2, 2, 8, 4, 8], // 农业银行
    [6, 2, 1, 6, 6, 0], // 建设银行
    [6, 2, 2, 5, 8, 0], // 中国银行
    [6, 2, 2, 5, 8, 8], // 交通银行
    [6, 2, 2, 1, 5, 5], // 招商银行
    [6, 2, 2, 6, 8, 9], // 中信银行
    [6, 2, 2, 6, 3, 0], // 华夏银行
    [6, 2, 2, 2, 6, 2], // 民生银行
    [6, 2, 2, 6, 6, 6], // 光大银行
    [6, 2, 1, 2, 8, 8], // 邮储银行
    [6, 2, 5, 9, 1, 2], // 中国平安银行
    [6, 2, 2, 3, 2, 3], // 兴业银行
];

pub fn generate_chinese_bank_card<T: rand::RngCore>(rng: &mut T) -> String {
    // 使用固定大小的数组，最大可能的卡号长度为19位
    let mut digits = [0u8; 19];

    // 随机选择一个银行BIN并复制到数组
    let bin_idx = rng.random_range(0..BANK_BINS.len());
    digits[..6].copy_from_slice(&BANK_BINS[bin_idx]);

    // 确定卡号总长度 (16-19位)
    let card_length = rng.random_range(16..=19) as usize;

    // 批量生成随机数字，使用单词随机调用生成多个数字
    // 这比每次生成一个数字要快得多
    let mut bulk_random = rng.random::<u64>() as u64;
    let mut shift = 0;

    // 生成中间部分的随机数字
    for i in 6..(card_length - 1) {
        if shift > 60 {
            // u64最多有64位，每次使用4位生成一个0-9的数字
            bulk_random = rng.random::<u64>() as u64;
            shift = 0;
        }

        digits[i] = ((bulk_random >> shift) & 0xF) as u8 % 10;
        shift += 4;
    }

    // 计算Luhn校验码
    digits[card_length - 1] = calculate_luhn_check_digit(&digits[..(card_length - 1)]);

    // 将数字数组转换为字符串 - 一次性分配内存
    let mut card_number = String::with_capacity(card_length);
    for &digit in &digits[..card_length] {
        card_number.push((digit + b'0') as char);
    }

    card_number
}

/// 计算Luhn校验算法的校验位 - 优化版本，直接操作u8数组
fn calculate_luhn_check_digit(digits: &[u8]) -> u8 {
    let mut sum = 0u16; // 使用u16避免可能的溢出
    let mut is_odd_position = digits.len() % 2 == 0;

    // 从左到右遍历数字 (对于Luhn算法来说，从右到左更常见，但我们可以根据位置的奇偶性反转规则)
    for &digit in digits {
        if is_odd_position {
            // 双倍奇数位置，如果结果大于9，则减去9
            let doubled = digit << 1; // 乘以2
            sum += if doubled > 9 { doubled - 9 } else { doubled } as u16;
        } else {
            // 偶数位置不变
            sum += digit as u16;
        }
        is_odd_position = !is_odd_position;
    }

    // 校验位是使总和能被10整除的数字
    ((10 - (sum % 10)) % 10) as u8
}
