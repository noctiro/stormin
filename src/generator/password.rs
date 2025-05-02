use rand::{Rng, RngCore};

// ---------- Chinese社工密码生成器 ----------
const COMMON_SURNAMES: &[&str] = &[
    "zhang", "li", "wang", "zhao", "liu", "chen", "yang", "huang", "wu", "xu",
    "sun", "zhou", "gao", "lin", "he", "ma", "luo", "zheng", "xie", "ye",
    "xu", "jiang", "tang", "liu", "xu", "shen", "wu", "xu", "song", "wei", 
    "pu", "xie", "zhu", "peng", "zhang", "yuan", "wu", "pan", "zhuang", 
];

const COMMON_GIVEN_NAMES: &[&str] = &[
    "wei", "fang", "min", "hua", "lei", "jing", "yan", "ting", "hao", "jun",
    "qiang", "ying", "li", "ping", "mei", "lin", "fei", "yun", "chao", "bo",
    "rong", "kai", "hao", "lei", "dong", "xia", "chen", "yu", "jie", "bin",
    "qi", "meng", "ya", "han", "rui", "feng", "gang", "liang", "xue", "wen",
    "jing", "ning", "jiao", "dong", "shan", "jing", "jiayi", "tian", "xian", 
    "yun", "fang", "chen", "chu", "lu", "an", "mei", "jun", "hao", "fang",
    "qi", "ling", "jun", "xuan", "shuang", "zheng", "tian", "pei", "ling",
    "xin", "xiao", "he", "rui", "ru", "xiang", "wen", "mu", "tao", "qiao",
    "lian", "hu", "shuang", "zhi", "xiang", "wei", "miao", "yan", "ting", 
    "ling", "su", "lai", "wen", "rong", "jia", "qi", "qiang", "zhi", "dong",
    "zhen", "yue", "xinyi", "xiaoyu", "luo", "zixuan", "huili", "xinyu", "wenjing", 
    "kaixin", "jiayi", "yichen", "yanli", "jiaqi", "ziwen", "yizhou", "sihan", "zihan",
    "yuxi", "jingxuan", "xinyue", "junwei", "yumin", "meilin", "chong", "xiangying",
    "wenhao", "yuxin", "jiayuan", "yutong", "linli", "liying", "yunfei", "yueqin",
    "chang", "zhaoyang", "xueqin", "chenyi", "jiahao", "haoyang", "lan", "liwei",
];

// 全拼或者首字母
fn pick_form<'a, T: RngCore>(s: &'a str, rng: &mut T) -> &'a str {
    if rng.random_bool(0.5) { s } else { &s[0..1] }
}

fn generate_name<T: RngCore>(rng: &mut T) -> String {
    let surname = COMMON_SURNAMES[rng.random_range(0..COMMON_SURNAMES.len())];
    let name_len = match rng.random_range(0..10) {
        0 => 2, 1 => 4, _ => 3,
    };
    let mut name = String::with_capacity(12);
    name.push_str(pick_form(surname, rng));
    for _ in 0..(name_len - 1) {
        let ch = COMMON_GIVEN_NAMES[rng.random_range(0..COMMON_GIVEN_NAMES.len())];
        name.push_str(pick_form(ch, rng));
    }
    name
}

fn generate_birthday<T: RngCore>(rng: &mut T) -> String {
    let year = rng.random_range(1970..=2010);
    let month = rng.random_range(1..=12);
    let day = rng.random_range(1..=28);
    let full = format!("{:04}{:02}{:02}", year, month, day);
    let short = &full[2..8];
    let month_day = &full[4..8];

    match rng.random_range(0..5) {
        0 => "".to_string(),
        1 => full,
        2 => short.to_string(),
        3 => month_day.to_string(),
        _ => format!("{}{}", &short[0..2], &short[2..]),
    }
}

fn generate_chinese_password<T: RngCore>(rng: &mut T) -> String {
    let name = generate_name(rng);
    let bday = generate_birthday(rng);
    if rng.random_bool(0.5) {
        format!("{}{}", name, bday)
    } else {
        format!("{}{}", bday, name)
    }
}

// ---------- 强密码生成器 ----------

fn generate_strong_password<T: RngCore>(rng: &mut T) -> String {
    const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const DIGITS: &[u8] = b"0123456789";
    const SYMBOLS: &[u8] = b"!@#$%^&*_-";

    let mut pool = Vec::with_capacity(80);
    pool.extend_from_slice(LETTERS);
    pool.extend_from_slice(DIGITS);
    if rng.random_bool(0.05) {
        pool.extend_from_slice(SYMBOLS);
    }

    let len = rng.random_range(8..=16);
    (0..len)
        .map(|_| {
            let idx = rng.random_range(0..pool.len());
            pool[idx] as char
        })
        .collect()
}

pub fn generate_password<T: RngCore>(rng: &mut T) -> String {
    if rng.random_bool(0.5) {
        generate_chinese_password(rng)
    } else {
        generate_strong_password(rng)
    }
}
