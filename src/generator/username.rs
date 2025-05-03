use rand::RngCore;
use std::fmt::Write;

static ADJECTIVES: &[&str] = &[
    "silent", "dark", "swift", "happy", "crazy", "cool", "smart", "lucky", "brave", "epic",
    "fuzzy", "wild", "mighty", "frozen", "hot", "shiny", "stealthy", "stormy", "dusty", "fierce",
    "agile", "blazing", "booming", "bright", "bold", "calm", "crafty", "daring", "deadly", "electric",
    "endless", "exotic", "fiery", "gentle", "ghostly", "grand", "grateful", "hairy", "hungry", "icy",
    "jealous", "jolly", "keen", "lofty", "magical", "massive", "modern", "mortal", "narrow", "noble",
    "ominous", "peaceful", "perfect", "primal", "proud", "radiant", "rapid", "reckless", "royal", "sacred",
    "sealed", "secret", "shallow", "slippery", "smoldering", "solitary", "soaring", "soft", "somber", "sparkling",
    "splendid", "spontaneous", "subtle", "towering", "tranquil", "untamed", "urban", "valiant", "vibrant", "vicious",
    "vigilant", "vivid", "volatile", "wandering", "weary", "wicked", "zany",
];

static NOUNS: &[&str] = &[
    "ninja", "tiger", "wizard", "dragon", "phoenix", "hacker", "ghost", "agent", "knight", "lion",
    "eagle", "viper", "wolf", "panther", "raven", "hunter", "mage", "rogue", "warrior", "pirate",
    "astronaut", "artist", "assassin", "barbarian", "beast", "bishop", "blade", "bomber", "broker", "captain",
    "cardinal", "champion", "chameleon", "conqueror", "counselor", "courier", "crusader", "dealer", "detective", "diplomat",
    "diva", "duke", "emperor", "engineer", "explorer", "fanatic", "farmer", "fighter", "gunslinger", "guardian",
    "judge", "kingmaker", "legislator", "liberator", "maestro", "merchant", "messenger", "monarch", "nomad", "officer",
    "overlord", "paladin", "peasant", "pilot", "prisoner", "prophet", "protector", "ranger", "rebel", "referee",
    "scout", "senator", "shepherd", "soldier", "spectator", "strategist", "survivor", "titan", "troubadour", "tyrant",
    "vanguard", "villager", "wanderer", "wizard", "zookeeper",
];

static VERBS: &[&str] = &[
    "run", "fly", "hack", "jump", "dash", "spin", "strike", "burn", "sneak", "zoom",
    "blaze", "bounce", "break", "build", "charge", "climb", "crush", "dive", "drift", "drive",
    "echo", "explore", "float", "forge", "freeze", "glide", "grind", "growl", "ignite", "lash",
    "leap", "march", "melt", "mine", "mix", "morph", "pack", "paint", "power", "prove",
    "quake", "race", "rebel", "reform", "resist", "rip", "roll", "rush", "scan", "sail",
    "scheme", "sculpt", "search", "shock", "shoot", "show", "slide", "soar", "spark", "split",
    "spread", "stamp", "steal", "storm", "surf", "swing", "tackle", "teleport", "thunder", "track",
    "transform", "twirl", "unite", "vanish", "venture", "weave",
];

static TECH_WORDS: &[&str] = &[
    "byte", "bit", "net", "cloud", "data", "cyber", "matrix", "script", "code", "dev",
    "ai", "bot", "exe", "dll", "io", "sys", "core", "spark", "pulse", "flux",
    "algorithm", "api", "array", "backup", "binary", "bluetooth", "buffer", "cache", "chipset", "client",
    "cluster", "compiler", "console", "cookie", "crypto", "debug", "desktop", "digital", "disk", "domain",
    "driver", "dynamic", "ethernet", "firewall", "firmware", "gateway", "graphql", "gpu", "gui", "hash",
    "http", "https", "identity", "index", "interface", "kernel", "library", "logic", "machine", "memory",
    "monitor", "module", "network", "node", "object", "packet", "parser", "platform", "protocol", "query",
    "queue", "router", "runtime", "schema", "server", "socket", "source", "storage", "syntax", "token",
    "transistor", "udp", "unicode", "usb", "variable", "virtual", "widget", "xml", "xpath", "zip",
];

static COMMON_SUFFIXES: &[&str] = &[
    "x", "z", "gg", "wp", "ez", "lol", "dev", "io", "bot", "pro",
    "exe", "sys", "app", "net", "master", "slayer", "killer", "walker", "rider", "one",
    "007", "123", "2024", "88", "alpha", "beta", "gamma", "delta", "omega", "king",
    "queen", "star", "hero", "boss",
    "amplify", "arc", "arrow", "atom", "aura", "blade", "blast", "bloom", "cafe", "calm",
    "chase", "charm", "circuit", "clique", "crest", "dash", "deed", "design", "drift", "edge",
    "elite", "era", "factor", "force", "gauge", "gem", "grid", "groove", "halo", "harmony",
    "hive", "icon", "impact", "link", "loop", "magic", "marvel", "mesh", "mode", "motif",
    "nexus", "orbit", "pace", "path", "phase", "portal", "rally", "realm", "saga", "scale",
    "shift", "stash", "surge", "swirl", "trance", "trend", "venture", "vibe", "vista", "zone",
];

pub fn generate_username<T: RngCore>(rng: &mut T) -> String {
    let mut result = String::with_capacity(32);
    let bits = rng.next_u64();

    let adj = ADJECTIVES[(bits & 0xFF) as usize % 20];
    let noun = NOUNS[((bits >> 8) & 0xFF) as usize % 20];
    let verb = VERBS[((bits >> 16) & 0xFF) as usize % 10];
    let tech = TECH_WORDS[((bits >> 24) & 0xFF) as usize % 20];
    let suffix = COMMON_SUFFIXES[((bits >> 32) & 0xFF) as usize % 35];

    let style = ((bits >> 40) & 0x07) as u8;       // 3 bits
    let use_verb = ((bits >> 43) & 0x01) != 0;
    let use_tech = ((bits >> 44) & 0x01) != 0;
    let add_digits = ((bits >> 45) & 0x01) != 0;
    let number = 10 + (((bits >> 46) & 0x3FF) % 90); // 10-99

    let (w1, w2) = if use_verb {
        (verb, noun)
    } else if use_tech {
        (adj, tech)
    } else {
        (adj, noun)
    };

    match style {
        0 => { // Plain
            result.push_str(w1);
            result.push_str(w2);
        }
        1 => { // CamelCase (manual cap)
            capitalize_into(w1, &mut result);
            capitalize_into(w2, &mut result);
        }
        2 => { // Reversed
            result.push_str(w2);
            result.push_str(w1);
        }
        3 => { // Dashed
            result.push_str(w1);
            result.push('-');
            result.push_str(w2);
        }
        4 => { // Repeated
            result.push_str(w1);
            result.push_str(w2);
            result.push_str(w2);
        }
        _ => { // Fallback: plain
            result.push_str(w1);
            result.push_str(w2);
        }
    }

    result.push_str(suffix);
    if add_digits {
        let _ = write!(result, "{}", number);
    }

    result
}

// inplace capitalization into a mutable String
fn capitalize_into(s: &str, buf: &mut String) {
    let mut chars = s.chars();
    if let Some(first) = chars.next() {
        buf.push(first.to_ascii_uppercase());
        buf.push_str(chars.as_str());
    }
}
