//! 8x8 bitmap font ported from olcPixelGameEngine (`olc_ConstructFontSheet`),
//! so text looks the same as the olcNES Part#2 CPU demo.

pub const SHEET_W: usize = 128;
pub const SHEET_H: usize = 48;

const DATA: &str = concat!(
    "?Q`0001oOch0o01o@F40o0<AGD4090LAGD<090@A7ch0?00O7Q`0600>00000000",
    "O000000nOT0063Qo4d8>?7a14Gno94AA4gno94AaOT0>o3`oO400o7QN00000400",
    "Of80001oOg<7O7moBGT7O7lABET024@aBEd714AiOdl717a_=TH013Q>00000000",
    "720D000V?V5oB3Q_HdUoE7a9@DdDE4A9@DmoE4A;Hg]oM4Aj8S4D84@`00000000",
    "OaPT1000Oa`^13P1@AI[?g`1@A=[OdAoHgljA4Ao?WlBA7l1710007l100000000",
    "ObM6000oOfMV?3QoBDD`O7a0BDDH@5A0BDD<@5A0BGeVO5ao@CQR?5Po00000000",
    "Oc``000?Ogij70PO2D]??0Ph2DUM@7i`2DTg@7lh2GUj?0TO0C1870T?00000000",
    "70<4001o?P<7?1QoHg43O;`h@GT0@:@LB@d0>:@hN@L0@?aoN@<0O7ao0000?000",
    "OcH0001SOglLA7mg24TnK7ln24US>0PL24U140PnOgl0>7QgOcH0K71S0000A000",
    "00H00000@Dm1S007@DUSg00?OdTnH7YhOfTL<7Yh@Cl0700?@Ah0300700000000",
    "<008001QL00ZA41a@6HnI<1i@FHLM81M@@0LG81?O`0nC?Y7?`0ZA7Y300080000",
    "O`082000Oh0827mo6>Hn?Wmo?6HnMb11MP08@C11H`08@FP0@@0004@000000000",
    "00P00001Oab00003OcKP0006@6=PMgl<@440MglH@000000`@000001P00000000",
    "Ob@8@@00Ob@8@Ga13R@8Mga172@8?PAo3R@827QoOb@820@0O`0007`0000007P0",
    "O`000P08Od400g`<3V=P0G`673IP0`@3>1`00P@6O`P00g`<O`000GP800000000",
    "?P9PL020O`<`N3R0@E4HC7b0@ET<ATB0@@l6C4B0O`H3N7b0?P01L3R000000020",
);

/// Decodes the packed font data into a 128x48 sheet of on/off pixels,
/// indexed `[y * SHEET_W + x]`. Glyphs for ASCII 32..127 are laid out
/// 16 per row, 8x8 pixels each.
pub fn build_sheet() -> Vec<bool> {
    let data = DATA.as_bytes();
    let mut sheet = vec![false; SHEET_W * SHEET_H];
    let (mut px, mut py) = (0, 0);
    for chunk in data.chunks(4) {
        let r = chunk
            .iter()
            .fold(0u32, |acc, &b| (acc << 6) | (b as u32 - 48));
        for i in 0..24 {
            sheet[py * SHEET_W + px] = r & (1 << i) != 0;
            py += 1;
            if py == SHEET_H {
                px += 1;
                py = 0;
            }
        }
    }
    sheet
}
