use unicode_normalization::UnicodeNormalization;

const FINA: usize = 0;
const INIT: usize = 1;
const MEDI: usize = 2;
const ISO: usize = 3;

// fina, init, medi, iso
const ARABIC_POSITION: [[u32; 4]; 42] = [
    [0xfe80, 0xfe80, 0xfe80, 0xfe80], // 0x621
    [0xfe82, 0xfe81, 0xfe82, 0xfe81],
    [0xfe84, 0xfe83, 0xfe84, 0xfe83],
    [0xfe86, 0xfe85, 0xfe86, 0xfe85],
    [0xfe88, 0xfe87, 0xfe88, 0xfe87],
    [0xfe8a, 0xfe8b, 0xfe8c, 0xfe89],
    [0xfe8e, 0xfe8d, 0xfe8e, 0xfe8d],
    [0xfe90, 0xfe91, 0xfe92, 0xfe8f], // 0x628
    [0xfe94, 0xfe93, 0xfe93, 0xfe93],
    [0xfe96, 0xfe97, 0xfe98, 0xfe95], // 0x62A
    [0xfe9a, 0xfe9b, 0xfe9c, 0xfe99],
    [0xfe9e, 0xfe9f, 0xfea0, 0xfe9d],
    [0xfea2, 0xfea3, 0xfea4, 0xfea1],
    [0xfea6, 0xfea7, 0xfea8, 0xfea5],
    [0xfeaa, 0xfea9, 0xfeaa, 0xfea9],
    [0xfeac, 0xfeab, 0xfeac, 0xfeab], // 0x630
    [0xfeae, 0xfead, 0xfeae, 0xfead],
    [0xfeb0, 0xfeaf, 0xfeb0, 0xfeaf],
    [0xfeb2, 0xfeb3, 0xfeb4, 0xfeb1],
    [0xfeb6, 0xfeb7, 0xfeb8, 0xfeb5],
    [0xfeba, 0xfebb, 0xfebc, 0xfeb9],
    [0xfebe, 0xfebf, 0xfec0, 0xfebd],
    [0xfec2, 0xfec3, 0xfec4, 0xfec1],
    [0xfec6, 0xfec7, 0xfec8, 0xfec5], // 0x638
    [0xfeca, 0xfecb, 0xfecc, 0xfec9],
    [0xfece, 0xfecf, 0xfed0, 0xfecd], //0x63A
    [0x63b, 0x63b, 0x63b, 0x63b],
    [0x63c, 0x63c, 0x63c, 0x63c],
    [0x63d, 0x63d, 0x63d, 0x63d],
    [0x63e, 0x63e, 0x63e, 0x63e],
    [0x63f, 0x63f, 0x63f, 0x63f],
    [0x640, 0x640, 0x640, 0x640], // 0x640
    [0xfed2, 0xfed3, 0xfed4, 0xfed1],
    [0xfed6, 0xfed7, 0xfed8, 0xfed5],
    [0xfeda, 0xfedb, 0xfedc, 0xfed9],
    [0xfede, 0xfedf, 0xfee0, 0xfedd],
    [0xfee2, 0xfee3, 0xfee4, 0xfee1],
    [0xfee6, 0xfee7, 0xfee8, 0xfee5],
    [0xfeea, 0xfeeb, 0xfeec, 0xfee9],
    [0xfeee, 0xfeed, 0xfeee, 0xfeed], // 0x648
    [0xfef0, 0xfeef, 0xfef0, 0xfeef],
    [0xfef2, 0xfef3, 0xfef4, 0xfef1], // 0x64A
];

const SPECIAL_BEHIND: [u32; 4] = [0x622, 0x623, 0x625, 0x627];

// 0x622，0x623，0x625，0x627
const ARABIC_SPECIAL: [[u32; 2]; 4] = [
    [0xFEF5, 0xFEF6],
    [0xFEF7, 0xFEF8],
    [0xFEF9, 0xFEFA],
    [0xFEFB, 0xFEFC],
];

const ARABIC_FRONT_SET: [u32; 23] = [
    0x62c, 0x62d, 0x62e, 0x647, 0x639, 0x63a, 0x641, 0x642, 0x62b, 0x635, 0x636, 0x637, 0x643,
    0x645, 0x646, 0x62a, 0x644, 0x628, 0x64a, 0x633, 0x634, 0x638, 0x626,
];

static ARABIC_BEHIND_SET: [u32; 35] = [
    0x62c, 0x62d, 0x62e, 0x647, 0x639, 0x63a, 0x641, 0x642, 0x62b, 0x635, 0x636, 0x637, 0x643,
    0x645, 0x646, 0x62a, 0x644, 0x628, 0x64a, 0x633, 0x634, 0x638, 0x626, 0x627, 0x623, 0x625,
    0x622, 0x62f, 0x630, 0x631, 0x632, 0x648, 0x624, 0x629, 0x649,
];

#[inline]
fn is_in_front_set(target: &u32) -> bool {
    ARABIC_FRONT_SET.contains(target)
}

#[inline]
fn is_in_behind_set(target: &u32) -> bool {
    ARABIC_BEHIND_SET.contains(target)
}

#[inline]
fn need_ligatures(target: u32) -> bool {
    (0x62..=0x64A).contains(&target)
}

/// we think zero width char will not influence ligatures
#[inline]
fn zero_width_char(target: u32) -> bool {
    (0x610..=0x61A).contains(&target)
        || (0x64B..=0x65F).contains(&target)
        || target >= 0x670
        || (0x6D6..=0x6ED).contains(&target)
}

fn do_ligatures(curr: u32, front: Option<char>, behind: Option<char>) -> u32 {
    let front = front.map(|x| x as u32).unwrap_or(0);
    let behind = behind.map(|x| x as u32).unwrap_or(0);

    let curr_index = (curr - 0x621) as usize;
    if is_in_front_set(&front) && is_in_behind_set(&behind) {
        // medi
        ARABIC_POSITION[curr_index][MEDI]
    } else if is_in_front_set(&front) && !is_in_behind_set(&behind) {
        ARABIC_POSITION[curr_index][FINA]
    } else if !is_in_front_set(&front) && is_in_behind_set(&behind) {
        ARABIC_POSITION[curr_index][INIT]
    } else {
        ARABIC_POSITION[curr_index][ISO]
    }
}

#[inline]
fn is_special_char(target: u32, behind: Option<char>) -> bool {
    let behind = behind.map(|x| x as u32).unwrap_or(0);
    target == 0x644 && SPECIAL_BEHIND.contains(&behind)
}

// 0x622，0x623，0x625，0x627
fn handle_special_char(front: Option<char>, behind: Option<char>) -> u32 {
    let front = front.map(|x| x as u32).unwrap_or(0);
    let behind = behind.map(|x| x as u32).unwrap_or(0);

    match behind {
        0x622 => {
            if is_in_front_set(&front) {
                ARABIC_SPECIAL[0][1]
            } else {
                ARABIC_SPECIAL[0][0]
            }
        }
        0x623 => {
            if is_in_front_set(&front) {
                ARABIC_SPECIAL[1][1]
            } else {
                ARABIC_SPECIAL[1][0]
            }
        }
        0x625 => {
            if is_in_front_set(&front) {
                ARABIC_SPECIAL[2][1]
            } else {
                ARABIC_SPECIAL[2][0]
            }
        }
        0x627 => {
            if is_in_front_set(&front) {
                ARABIC_SPECIAL[3][1]
            } else {
                ARABIC_SPECIAL[3][0]
            }
        }
        _ => 0x644,
    }
}

pub fn fix_arabic_ligatures_char(text: &str) -> String {
    let mut res = String::new();
    let mut vowel = String::new();
    let mut iter = text.nfc();

    let mut front = None;
    let mut behind = iter.next();
    let mut curr = behind;

    while curr.is_some() {
        if !vowel.is_empty() {
            res.push_str(&vowel);
            vowel = String::new();
        }
        let cha = curr.unwrap();
        let cha_usize = cha as u32;
        behind = iter.next();
        // if next is vowel, jump to next and remark this char
        if let Some(x) = behind {
            if zero_width_char(x as u32) {
                vowel.push(x);
                behind = iter.next();
            }
        }

        if need_ligatures(cha_usize) {
            // special ligatures 0x644
            if is_special_char(cha_usize, behind) {
                res.push(
                    std::char::from_u32(handle_special_char(front, behind))
                        .or_else(|| {
                            println!("ERROR: arabic char is not exist");
                            Some(' ')
                        })
                        .unwrap_or(' '),
                );
                curr = behind;
                behind = iter.next();
            } else {
                res.push(
                    std::char::from_u32(do_ligatures(cha_usize, front, behind))
                        .or_else(|| {
                            println!("ERROR: arabic char is not exist");
                            Some(' ')
                        })
                        .unwrap_or(' '),
                );
            }
        } else {
            res.push(cha);
        }
        front = curr;
        curr = behind;
    }

    if !vowel.is_empty() {
        res.push_str(&vowel);
    }
    res
}
