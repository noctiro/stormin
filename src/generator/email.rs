use super::username;
use rand::{Rng, RngCore};

const COMMON_SERVER: &[&str] = &[
    // Global
    "gmail.com", "googlemail.com", "outlook.com", "hotmail.com", "live.com", "yahoo.com", "aol.com",
    "icloud.com", "mail.com", "protonmail.com", "zoho.com", "gmx.com", "yandex.com", "msn.com", "me.com",

    // Chinese
    "qq.com", "vip.qq.com", "foxmail.com",
    "163.com", "vip.163.com", "126.com", "yeah.net",
    "sina.com", "sina.cn", "sohu.com",
    "aliyun.com", "aliyun.cn", "taobao.com",
    "139.com", "189.cn", "wo.cn",
];

pub fn generate_email<T: RngCore>(rng: &mut T) -> String {
    let username = username::generate_username(rng);
    let server = COMMON_SERVER[rng.random_range(0..COMMON_SERVER.len())];
    format!("{}@{}", username, server)
}
