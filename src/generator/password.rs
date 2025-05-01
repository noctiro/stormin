use rand::{Rng, SeedableRng};
use rand::rngs::{SmallRng, ThreadRng};

pub trait PasswordGenerator {
    fn generate(&mut self) -> String;
}

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

pub struct ChineseSocialPasswordGenerator {
    rng: SmallRng,
}

impl ChineseSocialPasswordGenerator {
    pub fn new() -> Self {
        Self {
            rng: SmallRng::seed_from_u64(ThreadRng::default().random()),
        }
    }

    fn pick_form<'a>(&mut self, s: &'a str) -> &'a str {
        if self.rng.random_bool(0.5) { s } else { &s[0..1] }
    }

    fn generate_name(&mut self) -> String {
        let surname = COMMON_SURNAMES[self.rng.random_range(0..COMMON_SURNAMES.len())];
        let name_len = match self.rng.random_range(0..10) {
            0 => 2, 1 => 4, _ => 3,
        };
        let mut name = String::with_capacity(12);
        name.push_str(self.pick_form(surname));
        for _ in 0..(name_len - 1) {
            let ch = COMMON_GIVEN_NAMES[self.rng.random_range(0..COMMON_GIVEN_NAMES.len())];
            name.push_str(self.pick_form(ch));
        }
        name
    }

    fn generate_birthday(&mut self) -> String {
        let year = self.rng.random_range(1970..=2010);
        let month = self.rng.random_range(1..=12);
        let day = self.rng.random_range(1..=28);
        let full = format!("{:04}{:02}{:02}", year, month, day);
        let short = &full[2..8];
        let month_day = &full[4..8];

        match self.rng.random_range(0..5) {
            0 => "".to_string(),
            1 => full,
            2 => short.to_string(),
            3 => month_day.to_string(),
            _ => format!("{}{}", &short[0..2], &short[2..]),
        }
    }
}

impl PasswordGenerator for ChineseSocialPasswordGenerator {
    fn generate(&mut self) -> String {
        let name = self.generate_name();
        let bday = self.generate_birthday();
        if self.rng.random_bool(0.5) {
            format!("{}{}", name, bday)
        } else {
            format!("{}{}", bday, name)
        }
    }
}

// ---------- 强密码生成器 ----------
pub struct RandomPasswordGenerator {
    rng: SmallRng,
}

impl RandomPasswordGenerator {
    pub fn new() -> Self {
        Self {
            rng: SmallRng::seed_from_u64(ThreadRng::default().random()),
        }
    }
}

impl PasswordGenerator for RandomPasswordGenerator {
    fn generate(&mut self) -> String {
        const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
        const DIGITS: &[u8] = b"0123456789";
        const SYMBOLS: &[u8] = b"!@#$%^&*_-";

        let mut pool = Vec::with_capacity(80);
        pool.extend_from_slice(LETTERS);
        pool.extend_from_slice(DIGITS);
        if self.rng.random_bool(0.05) {
            pool.extend_from_slice(SYMBOLS);
        }

        let len = self.rng.random_range(8..=16);
        (0..len)
            .map(|_| {
                let idx = self.rng.random_range(0..pool.len());
                pool[idx] as char
            })
            .collect()
    }
}
