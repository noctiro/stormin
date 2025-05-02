use rand::RngCore;
use std::fmt::Write;

// ---------- Expanded Word Lists (Place the expanded lists from above here) ----------

static COMMON_WORDS: &[&str] = &[
    "master", "ninja", "shadow", "agent", "alpha", "omega", "delta", "sigma", "gamma", "epic",
    "legend", "mythic", "cyber", "tech", "code", "hacker", "dev", "admin", "user", "player",
    "ghost", "viper", "eagle", "lion", "tiger", "wolf", "dragon", "phoenix", "wizard", "sorcerer",
    "knight", "warrior", "hunter", "rogue", "mage", "priest", "paladin", "joker", "ace", "jack",
    "pixel", "vector", "byte", "net", "web", "cloud", "data", "stream", "flux", "nova",
    "comet", "luna", "solar", "cosmic", "void", "rift", "spark", "bolt", "flash", "storm",
    "frost", "ember", "stone", "iron", "steel", "gold", "silver", "ruby", "jade", "onyx",
    "blue", "red", "green", "black", "white", "gray", "swift", "silent", "dark", "light",
    "prime", "ultra", "hyper", "meta", "guru", "sensei", "pilot", "captain", "rebel", "phantom",
    "zero", "one", "infinity", "apex", "zenith", "core", "matrix", "pulse", "echo", "origin",
    "cool", "love", "music", "game", "star", "dream", "lucky", "fast", "happy", "joy", "super",
    "power", "king", "queen", "hero", "champ", "fun", "best", "peace", "fire",
];

static COMMON_SUFFIXES: &[&str] = &[
    "x", "z", "gg", "wp", "ez", "xd", "lol", "rofl", "lmao", "brb", "afk", "btw", "fyi",
    "pro", "noob", "bot", "ai", "exe", "dll", "sys", "io", "dev", "ops", "sec", "net", "org",
    "com", "app", "xyz", "online", "live", "now", "go", "run", "fly", "win", "lost", "found",
    "master", "blaster", "slayer", "killer", "hacker", "tracker", "finder", "seeker", "walker", "rider",
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "zero", "prime",
    "alpha", "beta", "gamma", "delta", "omega", "sigma", "leet", "เทพ",
    "god", "demon", "angel", "spirit", "soul", "mind", "heart", "nova", "pulse", "spark", "wave",
    "123", "88", "007", "99", "2024", "king", "star", "love", "expert", "boss", "xiaoming",
    "lily", "1234", "abc", "superman", "haha", "cool", "fun", "good",
];

static POOL_LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
static POOL_DIGITS: &[u8] = b"0123456789";
static POOL_SYMBOLS: &[u8] = b"!@#$%^&*_-";

pub fn generate_username<T: RngCore>(rng: &mut T) -> String {
    // Allocate with reasonable capacity once
    let mut result = String::with_capacity(32);
    
    // Generate a single u64 and extract multiple random choices from it
    // This reduces the number of calls to the RNG
    let random_bits = rng.next_u64();
    
    // Use the bits to make our decisions
    // Extract bits 0-15 for word index (16 bits can represent 0-65535)
    let word_idx = (random_bits & 0xFFFF) as usize % COMMON_WORDS.len();
    let prefix_word = COMMON_WORDS[word_idx];
    
    // Use bit 16 for lowercase decision
    let lowercase_prefix = (random_bits & (1 << 16)) != 0;
    
    // Use bit 17 for underscore decision
    let add_underscore = (random_bits & (1 << 17)) != 0;
    
    // Extract bits 18-33 for suffix index
    let suffix_idx = ((random_bits >> 18) & 0xFFFF) as usize % COMMON_SUFFIXES.len();
    let suffix_base = COMMON_SUFFIXES[suffix_idx];
    
    // Use bit 34 for digits decision
    let add_suffix_digits = (random_bits & (1 << 34)) != 0;
    
    // Generate suffix number (bits 35-44, giving us 0-1023)
    // We'll scale it to 10-99 range
    let suffix_num = 10 + ((random_bits >> 35) & 0x3FF) % 90;
    
    // Now build the string with minimal allocations
    if lowercase_prefix {
        // Lowercase version
        for c in prefix_word.chars() {
            result.push(c.to_ascii_lowercase());
        }
    } else {
        // Original case
        result.push_str(prefix_word);
    }
    
    if add_underscore {
        result.push('_');
    }
    
    result.push_str(suffix_base);
    
    if add_suffix_digits {
        write!(result, "{}", suffix_num).unwrap();
    }
    
    result
}
