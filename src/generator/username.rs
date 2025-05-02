use rand::{Rng, RngCore};
use std::fmt::Write;

// ---------- Expanded Word Lists (Place the expanded lists from above here) ----------

const COMMON_WORDS: &[&str] = &[
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

const COMMON_SUFFIXES: &[&str] = &[
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

pub fn generate_username<T: RngCore>(rng: &mut T) -> String {
    // Estimate capacity: Longest word (e.g., 8) + underscore (1) + longest suffix (e.g., 8) + digits (2) = ~19.
    // Add some buffer. 32 seems reasonable.
    let mut result = String::with_capacity(32);

    // --- 1. Select and append prefix ---
    let prefix_word = COMMON_WORDS[rng.random_range(0..COMMON_WORDS.len())];
    if rng.random_bool(0.5) { // 50% chance lowercase prefix
        // Efficiently append lowercase version if needed
        // Using write! might be slightly cleaner than push+to_ascii_lowercase loop
        // write!(result, "{}", prefix_word.to_lowercase()).unwrap();
        // Let's benchmark push+loop vs write! later if needed, push+loop is likely fine
         for c in prefix_word.chars() {
             result.push(c.to_ascii_lowercase());
         }
    } else {
        result.push_str(prefix_word); // Append directly if no case change
    }

    // --- 2. Decide on separator ---
    let add_underscore = rng.random_bool(0.3); // 30% chance of underscore

    // --- 3. Select suffix base ---
    let suffix_base = COMMON_SUFFIXES[rng.random_range(0..COMMON_SUFFIXES.len())];

    // --- 4. Decide on adding numbers to suffix ---
    let add_suffix_digits = rng.random_bool(0.7); // 70% chance of adding digits

    // --- 5. Append separator (if needed) ---
    if add_underscore {
        result.push('_');
    }

    // --- 6. Append suffix base ---
    result.push_str(suffix_base);

    // --- 7. Append suffix digits (if needed) ---
    if add_suffix_digits {
        let suffix_num = rng.random_range(10..100);
        // Use write! for efficient formatting directly into the String buffer
        // write! returns a Result, unwrap is generally safe for basic types into String
        write!(result, "{}", suffix_num).unwrap();
    }

    result // Return the efficiently built string
}
