// The explore wall's lots, from the original `lotTables.ts`, `lotSeats.ts` and `exploreLayout.ts`.

/// `x, y, cols, rows` in cells.
pub type Seat = (i32, i32, i32, i32);

pub const LOT_COLS: i32 = 12;
pub const LOT_ROWS: i32 = 8;
/// `LOT_CAPACITY`: a longer section is dealt over several lots.
const LOT_CAPACITY: usize = 13;

const HEADING_STRIP: Seat = (0, 0, 6, 2);
const LEFT_SQUARE: Seat = (0, 2, 6, 6);
const RIGHT_SQUARE: Seat = (6, 0, 6, 6);
const RIGHT_STRIP: Seat = (6, 6, 6, 2);
const HEADING_HALF: Seat = (0, 0, 6, 8);

/// `SQUARE_SEATS[m]`: a 6 × 6 square cut into `m` seats.
const SQUARE_SEATS: [&[Seat]; 10] = [
    &[],
    &[(0, 0, 6, 6)],
    &[(0, 0, 6, 3), (0, 3, 6, 3)],
    &[(0, 0, 6, 4), (0, 4, 3, 2), (3, 4, 3, 2)],
    &[(0, 0, 4, 4), (4, 0, 2, 4), (0, 4, 4, 2), (4, 4, 2, 2)],
    &[(0, 0, 4, 4), (4, 0, 2, 2), (4, 2, 2, 2), (0, 4, 4, 2), (4, 4, 2, 2)],
    &[(0, 0, 4, 4), (4, 0, 2, 2), (4, 2, 2, 2), (0, 4, 2, 2), (2, 4, 2, 2), (4, 4, 2, 2)],
    &[(0, 0, 3, 2), (3, 0, 3, 2), (0, 2, 3, 2), (3, 2, 3, 2), (0, 4, 2, 2), (2, 4, 2, 2), (4, 4, 2, 2)],
    &[(0, 0, 3, 2), (3, 0, 3, 2), (0, 2, 2, 2), (2, 2, 2, 2), (4, 2, 2, 2), (0, 4, 2, 2), (2, 4, 2, 2), (4, 4, 2, 2)],
    &[
        (0, 0, 2, 2),
        (2, 0, 2, 2),
        (4, 0, 2, 2),
        (0, 2, 2, 2),
        (2, 2, 2, 2),
        (4, 2, 2, 2),
        (0, 4, 2, 2),
        (2, 4, 2, 2),
        (4, 4, 2, 2),
    ],
];

/// `STRIP_SEATS[m]`: a 6 × 2 strip cut into `m` seats.
const STRIP_SEATS: [&[Seat]; 4] =
    [&[], &[(0, 0, 6, 2)], &[(0, 0, 3, 2), (3, 0, 3, 2)], &[(0, 0, 2, 2), (2, 0, 2, 2), (4, 0, 2, 2)]];

/// The kinds on a lot stand collections first (`KIND_RANK`): albums, playlists, songs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    Album,
    Playlist,
    Song,
}

/// One section as the wall lays it: its key, its heading tile and its entry tiles in the page's order.
pub struct Section {
    pub key: String,
    pub heading: usize,
    pub entries: Vec<(usize, Rank)>,
}

/// One lot of the strip: the heading's seat and the entries on the other seats, `tiles[i]` on `seats[i]`.
#[derive(Debug, Clone)]
pub struct Lot {
    pub heading_tile: usize,
    pub heading: Seat,
    pub tiles: Vec<usize>,
    pub seats: Vec<Seat>,
    /// `mirrorsOf(section, part)`: how its two squares are mirrored.
    pub mirrors: u32,
}

/// `buildStrip`: every section's lots in order; a section is dealt evenly, earlier lots taking the extra entry.
pub fn strip(sections: &[Section]) -> Vec<Lot> {
    let mut lots = Vec::new();
    for section in sections.iter().filter(|section| !section.entries.is_empty()) {
        let total = section.entries.len();
        let parts = total.div_ceil(LOT_CAPACITY);
        let mut from = 0;
        for part in 0..parts {
            let size = total / parts + usize::from(part < total % parts);
            let mut entries = section.entries[from..from + size].to_vec();
            entries.sort_by_key(|(_, rank)| *rank);
            from += size;
            let mirrors = mirrors(&section.key, part);
            let (heading, seats) = cut(size, mirrors);
            lots.push(Lot {
                heading_tile: section.heading,
                heading,
                tiles: entries.into_iter().map(|(tile, _)| tile).collect(),
                seats,
                mirrors,
            });
        }
    }
    lots
}

/// `strideOf`: how many lots further on each column begins the strip.
pub fn stride(count: usize) -> i64 {
    if count >= 7 {
        3
    } else if count >= 4 {
        2
    } else {
        1
    }
}

/// `lotIndexAt`: the lot the place `(column, row)` holds; the plane repeats the strip without an edge.
pub fn lot_at(column: i64, row: i64, count: usize) -> usize {
    let count = count.max(1) as i64;
    (row + column * stride(count as usize)).rem_euclid(count) as usize
}

/// `mirrorsOf`: four mirror bits from the section key and part, FNV-1a over code points with JS 32-bit maths.
fn mirrors(key: &str, part: usize) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for character in format!("{key}#{part}").chars() {
        hash = (hash ^ u32::from(character)).wrapping_mul(0x0100_0193);
    }
    hash = (hash ^ (hash >> 15)).wrapping_mul(0x2c1b_3c6d);
    hash ^= hash >> 12;
    hash % 16
}

fn seats_in(field: Seat, table: &[Seat], mirror: u32) -> impl Iterator<Item = Seat> + '_ {
    table.iter().map(move |&(x, y, cols, rows)| {
        let x = if mirror & 1 == 1 { field.2 - x - cols } else { x };
        let y = if mirror & 2 == 2 { field.3 - y - rows } else { y };
        (field.0 + x, field.1 + y, cols, rows)
    })
}

/// `stripSeatsOf`.
fn strip_seats(count: usize) -> usize {
    if count <= 3 {
        1
    } else if count == LOT_CAPACITY {
        3
    } else {
        2
    }
}

/// `cutLot`: the heading's seat and the seats for `count` entries, largest first, then top to bottom, left to right.
fn cut(count: usize, mirrors: u32) -> (Seat, Vec<Seat>) {
    if count <= 2 {
        let seats = match count {
            1 => vec![(6, 0, 6, 8)],
            2 => vec![(6, 0, 6, 4), (6, 4, 6, 4)],
            _ => Vec::new(),
        };
        return (HEADING_HALF, seats);
    }
    let strip = strip_seats(count);
    let left = (count - strip) / 2;
    let right = count - strip - left;
    let mut seats: Vec<Seat> = seats_in(LEFT_SQUARE, SQUARE_SEATS[left.min(9)], mirrors & 3)
        .chain(seats_in(RIGHT_SQUARE, SQUARE_SEATS[right.min(9)], (mirrors >> 2) & 3))
        .chain(seats_in(RIGHT_STRIP, STRIP_SEATS[strip], 0))
        .collect();
    seats.sort_by(|a, b| (b.2 * b.3).cmp(&(a.2 * a.3)).then(a.1.cmp(&b.1)).then(a.0.cmp(&b.0)));
    (HEADING_STRIP, seats)
}

/// `grownStripSeatsOf`: strip seats for the tiles left beside a grown one.
fn grown_strip_seats(others: usize) -> usize {
    3.min((others.saturating_sub(9)).max(if others > 2 { 2 } else { 1 }))
}

/// `cutGrownLot`: the seats while the tile on `grown` takes a 6x6 square; the others move the least they can.
pub fn cut_grown(lot: &Lot, grown: usize) -> Vec<Seat> {
    let count = lot.seats.len();
    let base = &lot.seats;
    let Some(&target) = base.get(grown) else { return base.clone() };
    if count <= 2 {
        return (0..count).map(|seat| if seat == grown { RIGHT_SQUARE } else { RIGHT_STRIP }).collect();
    }
    let in_left = target.0 * 2 + target.2 < (LEFT_SQUARE.0 + LEFT_SQUARE.2) * 2;
    let others = count - 1;
    let strip = grown_strip_seats(others);
    let (square, mirror) =
        if in_left { (RIGHT_SQUARE, (lot.mirrors >> 2) & 3) } else { (LEFT_SQUARE, lot.mirrors & 3) };
    let free: Vec<Seat> = seats_in(square, SQUARE_SEATS[(others - strip).min(9)], mirror)
        .chain(seats_in(RIGHT_STRIP, STRIP_SEATS[strip], 0))
        .collect();
    let movers: Vec<usize> = (0..count).filter(|seat| *seat != grown).collect();
    let taken = least_travel(&movers.iter().map(|seat| base[*seat]).collect::<Vec<_>>(), &free);
    let mut result = base.clone();
    result[grown] = if in_left { LEFT_SQUARE } else { RIGHT_SQUARE };
    for (index, seat) in movers.iter().enumerate() {
        if let Some(place) = taken.get(index).and_then(|place| free.get(*place)) {
            result[*seat] = *place;
        }
    }
    result
}

/// `seatWithLeastTravel`: the assignment of tiles to seats with the least summed squared travel of the centres.
fn least_travel(from: &[Seat], to: &[Seat]) -> Vec<usize> {
    let count = from.len().min(to.len());
    let travel = |tile: Seat, seat: Seat| {
        f64::from(
            (2 * tile.0 + tile.2 - 2 * seat.0 - seat.2).pow(2) + (2 * tile.1 + tile.3 - 2 * seat.1 - seat.3).pow(2),
        )
    };
    let states = 1usize << count;
    let mut best = vec![f64::INFINITY; states];
    let mut last = vec![0usize; states];
    best[0] = 0.0;
    for mask in 0..states {
        let tile = mask.count_ones() as usize;
        if tile >= count {
            continue;
        }
        for seat in 0..count {
            if mask & (1 << seat) != 0 {
                continue;
            }
            let sum = best[mask] + travel(from[tile], to[seat]);
            if sum < best[mask | (1 << seat)] {
                best[mask | (1 << seat)] = sum;
                last[mask | (1 << seat)] = seat;
            }
        }
    }
    let mut taken = vec![0; count];
    let mut mask = states - 1;
    for tile in (0..count).rev() {
        taken[tile] = last[mask];
        mask &= !(1 << last[mask]);
    }
    taken
}
