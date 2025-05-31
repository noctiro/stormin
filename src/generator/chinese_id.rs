use rand::Rng;

pub fn generate_chinese_id<T: rand::RngCore>(rng: &mut T) -> String {
    // Pre-allocate a String with the exact capacity needed
    let mut id = String::with_capacity(18);

    // Fast region code generation with expanded geographic coverage
    // Using province+city prefixes and adding district numbers
    let region_code = match rng.random_range(0..30) {
        // North China
        0 => 110100 + rng.random_range(1..=16), // Beijing
        1 => 120100 + rng.random_range(1..=16), // Tianjin
        2 => 130100 + rng.random_range(1..=10), // Shijiazhuang, Hebei
        3 => 140100 + rng.random_range(1..=10), // Taiyuan, Shanxi

        // Northeast China
        4 => 210100 + rng.random_range(1..=13), // Shenyang, Liaoning
        5 => 220100 + rng.random_range(1..=9),  // Changchun, Jilin
        6 => 230100 + rng.random_range(1..=9),  // Harbin, Heilongjiang

        // East China
        7 => 310100 + rng.random_range(1..=16),  // Shanghai
        8 => 320100 + rng.random_range(1..=11),  // Nanjing, Jiangsu
        9 => 330100 + rng.random_range(1..=13),  // Hangzhou, Zhejiang
        10 => 340100 + rng.random_range(1..=9),  // Hefei, Anhui
        11 => 350100 + rng.random_range(1..=13), // Fuzhou, Fujian
        12 => 370100 + rng.random_range(1..=12), // Jinan, Shandong

        // South Central China
        13 => 410100 + rng.random_range(1..=12), // Zhengzhou, Henan
        14 => 420100 + rng.random_range(1..=13), // Wuhan, Hubei
        15 => 430100 + rng.random_range(1..=9),  // Changsha, Hunan
        16 => 440100 + rng.random_range(1..=12), // Guangzhou, Guangdong
        17 => 450100 + rng.random_range(1..=12), // Nanning, Guangxi
        18 => 460100 + rng.random_range(1..=7),  // Haikou, Hainan

        // Southwest China
        19 => 500100 + rng.random_range(1..=9),  // Chongqing
        20 => 510100 + rng.random_range(1..=12), // Chengdu, Sichuan
        21 => 520100 + rng.random_range(1..=10), // Guiyang, Guizhou
        22 => 530100 + rng.random_range(1..=14), // Kunming, Yunnan
        23 => 540100 + rng.random_range(1..=8),  // Lhasa, Tibet

        // Northwest China
        24 => 610100 + rng.random_range(1..=13), // Xi'an, Shaanxi
        25 => 620100 + rng.random_range(1..=8),  // Lanzhou, Gansu
        26 => 630100 + rng.random_range(1..=7),  // Xining, Qinghai
        27 => 640100 + rng.random_range(1..=9),  // Yinchuan, Ningxia
        28 => 650100 + rng.random_range(1..=8),  // Urumqi, Xinjiang

        // Special Administrative Regions
        _ => 810000 + rng.random_range(1..=18), // Hong Kong (Note: format differs in practice)
    };

    // Append region code to ID
    id.push_str(&region_code.to_string());

    // Generate birth date between 1950-01-01 and 2025-12-31
    // Fixed upper bound instead of getting current year for performance
    let year = rng.random_range(1950..=2025);
    let month = rng.random_range(1..=12);

    // Fast day calculation
    let max_day = match month {
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    let day = rng.random_range(1..=max_day);

    // Append birth date efficiently
    id.push_str(&format!("{:04}{:02}{:02}", year, month, day));

    // Generate sequence number and append
    id.push_str(&format!("{:03}", rng.random_range(1..=999)));

    // Calculate check digit
    // Weights and check mapping as constants for better performance
    const WEIGHTS: [u8; 17] = [7, 9, 10, 5, 8, 4, 2, 1, 6, 3, 7, 9, 10, 5, 8, 4, 2];
    const CHECK_MAPPING: [char; 11] = ['1', '0', 'X', '9', '8', '7', '6', '5', '4', '3', '2'];

    // Optimized weighted sum calculation, avoiding chars iteration
    let mut sum = 0;
    for i in 0..17 {
        // Direct byte access and conversion is faster than char iteration
        let digit = (id.as_bytes()[i] - b'0') as usize;
        sum += digit * WEIGHTS[i] as usize;
    }

    // Append check digit
    id.push(CHECK_MAPPING[sum % 11]);

    id
}
