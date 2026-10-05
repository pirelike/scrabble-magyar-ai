#![allow(dead_code)]

use pg_scrabble::board::{Board, Direction};
use pg_scrabble::lexicon::Lexicon;
use pg_scrabble::movegen::Play;
use pg_scrabble::rack::Rack;
use pg_scrabble::rules::GameConfig;
use pg_scrabble::tile::{Square, Tile};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PlayKey {
    pub placements: Vec<(usize, usize, Square)>,
    pub score: i32,
}

impl PlayKey {
    pub fn of(play: &Play) -> PlayKey {
        let mut placements: Vec<(usize, usize, Square)> =
            play.placements().map(|(c, s)| (c.row, c.col, s)).collect();
        placements.sort();
        PlayKey {
            placements,
            score: play.score(),
        }
    }

    pub fn describe(&self, config: &GameConfig) -> String {
        let cells: Vec<String> = self
            .placements
            .iter()
            .map(|&(r, c, s)| {
                let letter = config.alphabet.display(s.index_unchecked());
                let letter = if s.is_blank() {
                    letter.to_lowercase()
                } else {
                    letter.to_string()
                };
                format!("{}{}={}", r + 1, (b'A' + c as u8) as char, letter)
            })
            .collect();
        format!("[{}] = {}", cells.join(" "), self.score)
    }
}

pub fn generate(
    board: &Board,
    rack: &Rack,
    lexicon: &Lexicon,
    config: &GameConfig,
) -> Vec<PlayKey> {
    let mut out = Vec::new();
    let words = lexicon.dawg().collect_words();

    for dir in Direction::ALL {
        let lane_len = board.lane_len(dir);
        for word in &words {
            if word.len() < 2 || word.len() > lane_len {
                continue;
            }
            for lane in 0..board.lane_count(dir) {
                for start in 0..=(lane_len - word.len()) {
                    try_word(
                        board, rack, lexicon, config, dir, lane, start, word, &mut out,
                    );
                }
            }
        }
    }

    out.sort();
    out.dedup();
    out
}

#[allow(clippy::too_many_arguments)]
fn try_word(
    board: &Board,
    rack: &Rack,
    lexicon: &Lexicon,
    config: &GameConfig,
    dir: Direction,
    lane: usize,
    start: usize,
    word: &[u8],
    out: &mut Vec<PlayKey>,
) {
    let lane_len = board.lane_len(dir);
    let cells = board.lane(dir, lane);
    let end = start + word.len();

    if start > 0 && cells[start - 1].is_occupied() {
        return;
    }
    if end < lane_len && cells[end].is_occupied() {
        return;
    }

    let mut needed: Vec<(usize, u8)> = Vec::new();
    for (i, &letter) in word.iter().enumerate() {
        let cell = cells[start + i];
        if cell.is_occupied() {
            if cell.index_unchecked() != letter {
                return;
            }
        } else {
            needed.push((start + i, letter));
        }
    }
    if needed.is_empty() || needed.len() > rack.len() {
        return;
    }

    if !connects(board, config, dir, lane, start, end, &needed) {
        return;
    }

    let n = needed.len();
    for mask in 0u32..(1 << n) {
        if mask.count_ones() > rack.blanks() as u32 {
            continue;
        }
        let mut supply = *rack;
        let mut affordable = true;
        for (i, &(_, letter)) in needed.iter().enumerate() {
            let tile = if mask & (1 << i) != 0 {
                Tile::BLANK
            } else {
                Tile::letter(letter)
            };
            if !supply.remove(tile) {
                affordable = false;
                break;
            }
        }
        if !affordable {
            continue;
        }

        let placed: Vec<(usize, Square)> = needed
            .iter()
            .enumerate()
            .map(|(i, &(offset, letter))| {
                let square = if mask & (1 << i) != 0 {
                    Square::blank_letter(letter)
                } else {
                    Square::letter(letter)
                };
                (offset, square)
            })
            .collect();

        if !cross_words_are_real(board, lexicon, dir, lane, &placed) {
            continue;
        }

        let score = score(board, config, dir, lane, start, end, &placed);
        let mut placements: Vec<(usize, usize, Square)> = placed
            .iter()
            .map(|&(offset, square)| {
                let c = Board::from_lane(dir, lane, offset);
                (c.row, c.col, square)
            })
            .collect();
        placements.sort();
        out.push(PlayKey { placements, score });
    }
}

fn connects(
    board: &Board,
    config: &GameConfig,
    dir: Direction,
    lane: usize,
    start: usize,
    end: usize,
    needed: &[(usize, u8)],
) -> bool {
    if board.is_empty() {
        if !config.layout.start_required() {
            return true;
        }
        let (sr, sc) = config.layout.start();
        return needed.iter().any(|&(offset, _)| {
            let c = Board::from_lane(dir, lane, offset);
            (c.row, c.col) == (sr, sc)
        });
    }

    let cells = board.lane(dir, lane);
    if cells[start..end].iter().any(|c| c.is_occupied()) {
        return true;
    }

    needed.iter().any(|&(offset, _)| {
        let c = Board::from_lane(dir, lane, offset);
        board.has_neighbor(c.row, c.col)
    })
}

fn cross_words_are_real(
    board: &Board,
    lexicon: &Lexicon,
    dir: Direction,
    lane: usize,
    placed: &[(usize, Square)],
) -> bool {
    let cross_dir = dir.flip();
    for &(offset, square) in placed {
        let coord = Board::from_lane(dir, lane, offset);
        let (cl, co) = Board::to_lane(cross_dir, coord);
        let cells = board.lane(cross_dir, cl);

        let mut word = Vec::new();
        let mut i = co;
        while i > 0 && cells[i - 1].is_occupied() {
            i -= 1;
        }
        for cell in &cells[i..co] {
            word.push(cell.index_unchecked());
        }
        word.push(square.index_unchecked());
        let mut j = co + 1;
        while j < cells.len() && cells[j].is_occupied() {
            word.push(cells[j].index_unchecked());
            j += 1;
        }

        if word.len() > 1 && !lexicon.contains(&word) {
            return false;
        }
    }
    true
}

fn score(
    board: &Board,
    config: &GameConfig,
    dir: Direction,
    lane: usize,
    start: usize,
    end: usize,
    placed: &[(usize, Square)],
) -> i32 {
    let alphabet = &config.alphabet;
    let layout = &config.layout;
    let cells = board.lane(dir, lane);
    let cross_dir = dir.flip();

    let value = |s: Square| -> i32 {
        if s.is_blank() {
            0
        } else {
            alphabet.score(s.index_unchecked())
        }
    };

    let mut main = 0;
    let mut word_mult = 1;
    let mut cross_total = 0;

    #[allow(clippy::needless_range_loop)]
    for offset in start..end {
        match placed.iter().find(|&&(o, _)| o == offset) {
            None => main += value(cells[offset]),
            Some(&(_, square)) => {
                let coord = Board::from_lane(dir, lane, offset);
                let premium = layout.premium(coord.row, coord.col);
                let letter_score = value(square) * premium.letter_multiplier();
                main += letter_score;
                word_mult *= premium.word_multiplier();

                let (cl, co) = Board::to_lane(cross_dir, coord);
                let cross_cells = board.lane(cross_dir, cl);
                let mut cross = 0;
                let mut len = 1;
                let mut i = co;
                while i > 0 && cross_cells[i - 1].is_occupied() {
                    i -= 1;
                    cross += value(cross_cells[i]);
                    len += 1;
                }
                let mut j = co + 1;
                while j < cross_cells.len() && cross_cells[j].is_occupied() {
                    cross += value(cross_cells[j]);
                    len += 1;
                    j += 1;
                }
                if len > 1 {
                    cross_total += (cross + letter_score) * premium.word_multiplier();
                }
            }
        }
    }

    let mut total = main * word_mult + cross_total;
    if placed.len() >= config.rack_size {
        total += config.bingo_bonus;
    }
    total
}

pub const WORDS: &str = "\
AA AB AD AE AG AH AI AL AM AN AR AS AT AW AX AY BA BE BI BO BY DE DO ED EF EH
EL EM EN ER ES ET EX FA FE GO HA HE HI HM HO ID IF IN IS IT JO KA KI LA LI LO
MA ME MI MM MO MU MY NA NE NO NU OD OE OF OH OI OK OM ON OP OR OS OW OX OY PA
PE PI PO QI RE SH SI SO TA TI TO UH UM UN UP US UT WE WO XI XU YA YE YO ZA
ACE ACT ADD ADO AFT AGE AGO AID AIL AIM AIR ALE ALL ALP AMP AND ANT ANY APE APT
ARC ARE ARK ARM ART ASH ASK ATE AWE AXE AYE BAD BAG BAN BAR BAT BAY BED BEE BEG
BET BID BIG BIN BIT BOA BOB BOG BOW BOX BOY BRA BUD BUG BUN BUS BUT BUY CAB CAD
CAM CAN CAP CAR CAT CAW COB COD COG CON COO COP COT COW COY CRY CUB CUD CUE CUP
CUR CUT DAB DAD DAM DAY DEN DEW DID DIE DIG DIM DIN DIP DOE DOG DON DOT DRY DUB
DUD DUE DUG DUO DYE EAR EAT EBB EEL EGG EGO ELF ELK ELM EMU END EON ERA ERR EVE
EWE EYE FAD FAN FAR FAT FAX FED FEE FEW FIB FIG FIN FIR FIT FIX FLU FLY FOE FOG
FOR FOX FRY FUN FUR GAB GAD GAG GAL GAP GAS GEL GEM GET GIG GIN GNU GOB GOD GOO
GOT GUM GUN GUT GUY GYM HAD HAG HAM HAS HAT HAY HEM HEN HER HEW HEX HEY HID HIM
HIP HIS HIT HOB HOE HOG HOP HOT HOW HUB HUE HUG HUM HUT ICE ICY ILK ILL IMP INK
INN ION IRE IRK ITS IVY JAB JAG JAM JAR JAW JAY JET JIG JOB JOG JOT JOY JUG JUT
KEG KEY KID KIN KIT LAB LAD LAG LAM LAP LAW LAX LAY LEA LED LEG LET LID LIE LIP
LIT LOB LOG LOO LOP LOT LOW LUG MAD MAN MAP MAR MAT MAW MAY MEN MET MEW MID MIX
MOB MOD MOM MOP MOW MUD MUG MUM NAB NAG NAP NAY NET NEW NIB NIL NIP NIT NOD NOR
NOT NOW NUN NUT OAF OAK OAR OAT ODD ODE OFF OFT OIL OLD ONE OPT ORB ORE OUR OUT
OWE OWL OWN PAD PAL PAN PAP PAR PAT PAW PAY PEA PEG PEN PEP PER PET PEW PIE PIG
PIN PIP PIT PLY POD POT POW PRO PRY PUB PUG PUN PUP PUS PUT RAG RAM RAN RAP RAT
RAW RAY RED REF RIB RID RIG RIM RIP ROB ROD ROE ROT ROW RUB RUE RUG RUM RUN RUT
RYE SAC SAD SAG SAP SAT SAW SAX SAY SEA SEE SET SEW SHE SHY SIN SIP SIR SIT SIX
SKI SKY SLY SOB SOD SON SOP SOT SOW SOY SPA SPY STY SUB SUE SUM SUN SUP TAB TAD
TAG TAN TAP TAR TAT TAX TEA TEE TEN THE THY TIC TIE TIN TIP TOE TOG TON TOO TOP
TOT TOW TOY TRY TUB TUG TUN TUX TWO URN USE VAN VAT VET VIA VIE VOW WAD WAG WAN
WAR WAS WAX WAY WEB WED WEE WET WHO WHY WIG WIN WIT WOE WOK WON WOO WOW WRY YAK
YAM YAP YAW YEA YEN YES YET YEW YIN YIP YON YOU ZAG ZAP ZED ZIG ZIP ZOO
ABLE ACHE ACID ACRE AIDE AJAR AKIN ALSO AREA ARID ARMY AUNT AWAY AXES AXLE BABY
BACK BAIL BAIT BAKE BALD BALE BALL BAND BANE BANK BARE BARK BARN BASE BASH BASK
BATH BEAD BEAM BEAN BEAR BEAT BEEF BEEN BEER BELL BELT BEND BENT BEST BIAS BIDE
BIKE BILE BILL BIND BIRD BITE BLED BLEW BLOB BLOC BLOT BLOW BLUE BLUR BOAR BOAT
BODE BODY BOIL BOLD BOLT BOMB BOND BONE BOOK BOOM BOON BOOT BORE BORN BOSS BOTH
BOUT BOWL BRAG BRAN BRAT BRAY BRED BREW BRIM BROW BUCK BULB BULK BULL BUMP BUNK
BURN BURP BURY BUSH BUST BUSY CAFE CAGE CAKE CALF CALL CALM CAME CAMP CANE CAPE
CARD CARE CARP CART CASE CASH CASK CAST CAVE CEDE CELL CENT CHAP CHAR CHAT CHEF
CHEW CHIC CHIN CHIP CHOP CITE CITY CLAD CLAM CLAN CLAP CLAW CLAY CLIP CLOG CLOT
CLUB CLUE COAL COAT COCK CODE COIL COIN COKE COLD COLT COMB COME CONE COOK COOL
COPE COPY CORD CORE CORK CORN COST COVE COZY CRAB CRAG CRAM CREW CRIB CROP CROW
CUBE CUFF CULT CURB CURD CURE CURL CURT CUSP CYST DAILY DAIRY
DARE DARK DARN DART DASH DATA DATE DAWN DAYS DEAD DEAF DEAL DEAN DEAR DEBT DECK
DEED DEEM DEEP DEER DEFT DEFY DELL DENT DENY DESK DIAL DICE DIET DIME DINE DING
DIRE DIRT DISC DISH DISK DIVE DOCK DOES DOLE DOLL DOME DONE DOOM DOOR DOPE DORM
DOSE DOTE DOVE DOWN DOZE DRAB DRAG DRAM DRAW DREW DRIP DROP DRUG DRUM DUAL DUCK
DUCT DUEL DUKE DULL DULY DUMB DUMP DUNE DUSK DUST DUTY EACH EARL EARN EASE EAST
EASY EATS ECHO EDGE EDIT EELS EGGS EMIT ENDS EPIC EVEN EVER EVIL EXAM EXIT EYES
FACE FACT FADE FAIL FAIR FAKE FALL FAME FANG FARE FARM FAST FATE FAWN FEAR FEAT
FEED FEEL FEET FELL FELT FERN FEUD FILE FILL FILM FIND FINE FIRE FIRM FISH FIST
FIVE FLAG FLAP FLAT FLAW FLEA FLED FLEE FLEW FLEX FLIP FLOG FLOP FLOW FLUE FOAL
FOAM FOIL FOLD FOLK FOND FONT FOOD FOOL FOOT FORD FORE FORK FORM FORT FOUL FOUR
FOWL FRAY FREE FRET FROG FROM FUEL FULL FUME FUND FUSE FUSS GAIN GAIT GALA GALE
GAME GANG GAPE GASH GATE GAVE GAZE GEAR GENE GERM GIFT GILD GILL GIRL GIST GIVE
GLAD GLEE GLEN GLOW GLUE GNAT GOAD GOAL GOAT GOES GOLD GOLF GONE GONG GOOD GORE
GOWN GRAB GRAM GRAY GREW GREY GRID GRIM GRIN GRIP GRIT GROW GRUB GULF GULL GUST
CATS RATS BATS EATS TEAS SEAT EAST SETA ATES ETAS SATE TAES SEATS
STONE TONES NOTES ONSET SETON STENO
RETAIN RETINA RATINE";

pub fn word_list() -> Vec<&'static str> {
    WORDS.split_whitespace().collect()
}

pub fn lexicon(config: &GameConfig) -> Lexicon {
    Lexicon::from_words(&config.alphabet, word_list())
        .expect("the built-in test words are all plain A-Z")
        .with_name("test")
}
