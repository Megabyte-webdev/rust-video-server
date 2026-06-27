use nanoid::nanoid;

const ALPHABET: &[char] = &[
    'a',
    'b',
    'c',
    'd',
    'e',
    'f',
    'g',
    'h',
    'i',
    'j',
    'k',
    'l',
    'm',
    'n',
    'o',
    'p',
    'q',
    'r',
    's',
    't',
    'u',
    'v',
    'w',
    'x',
    'y',
    'z',
];

pub fn generate_room_id() -> String {
    let part1 = nanoid!(4, ALPHABET);
    let part2 = nanoid!(4, ALPHABET);
    let part3 = nanoid!(4, ALPHABET);

    format!("{}-{}-{}", part1, part2, part3)
}
