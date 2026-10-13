//! ROADMAP C2: every LUT parser checks a file-derived size against `vfx_lut::limits` (OCIO
//! `LutLimits.h`: 1D at most 300000 entries, 3D at most 129 per axis, both at least 2) before it
//! allocates or divides. Each case here is a header that used to reach an allocation, a division
//! by `size - 1` or a silent truncation; each must now be an `Err`, never a panic or an abort.

use std::io::Cursor;
use vfx_lut::{
    csp::parse_csp,
    cube::parse_cube,
    discreet1dl::parse_1dl,
    hdl::parse_hdl,
    iridas_itx::parse_itx,
    iridas_look::parse_look,
    nuke_vf::parse_vf,
    pandora::parse_mga,
    spi::{parse_spi1d, parse_spi3d},
    threedl::parse_3dl,
    truelight::parse_cub,
};

const BAD_3D: [&str; 5] = ["0", "1", "130", "4000000", "99999999999"];

fn rejected<T: std::fmt::Debug, E>(what: &str, r: Result<T, E>) {
    assert!(r.is_err(), "{what} must be rejected, got {:?}", r.ok());
}

#[test]
fn cube() {
    for n in BAD_3D {
        rejected(
            &format!("cube LUT_3D_SIZE {n}"),
            parse_cube(Cursor::new(format!("LUT_3D_SIZE {n}\n"))),
        );
    }
    for n in ["1", "300001"] {
        rejected(
            &format!("cube LUT_1D_SIZE {n}"),
            parse_cube(Cursor::new(format!("LUT_1D_SIZE {n}\n"))),
        );
    }
}

#[test]
fn itx() {
    for n in BAD_3D {
        rejected(
            &format!("itx {n}"),
            parse_itx(Cursor::new(format!("LUT_3D_SIZE {n}\n"))),
        );
    }
}

#[test]
fn iridas_look() {
    for n in BAD_3D {
        let xml =
            format!("<looks><look><LUT><size>\"{n}\"</size><data>\"\"</data></LUT></look></looks>");
        rejected(&format!("look {n}"), parse_look(Cursor::new(xml)));
    }
}

#[test]
fn hdl() {
    let head = "Version 2\nFormat any\nFrom 0.0 1.0\nTo 0.0 1.0\nBlack 0.0\nWhite 1.0\n";
    for n in BAD_3D {
        let f = format!("{head}Type 3D\nLength {n}\nLUT:\n {{\n 0 0 0\n }}\n");
        rejected(&format!("hdl 3D {n}"), parse_hdl(Cursor::new(f)));
    }
    for n in ["1", "300001"] {
        let f = format!("{head}Type C\nLength {n}\nLUT:\nRGB {{\n 0\n }}\n");
        rejected(&format!("hdl 1D {n}"), parse_hdl(Cursor::new(f)));
    }
    // A non-integer Length token used to be dropped silently.
    let f = format!("{head}Type 3D\nLength two\nLUT:\n {{\n 0 0 0\n }}\n");
    rejected("hdl Length two", parse_hdl(Cursor::new(f)));
    // 3D+1D needs the prelut size; it used to default to 1024.
    let f = format!("{head}Type 3D+1D\nLength 2\nLUT:\nPre {{\n 0\n 1\n }}\n {{\n 0 0 0\n }}\n");
    rejected(
        "hdl 3D+1D without the prelut size",
        parse_hdl(Cursor::new(f)),
    );
}

#[test]
fn csp() {
    let prelut = "2\n0 1\n0 1\n2\n0 1\n0 1\n2\n0 1\n0 1\n";
    for n in BAD_3D {
        let f = format!("CSPLUTV100\n3D\n\n{prelut}\n{n} {n} {n}\n");
        rejected(&format!("csp 3D {n}"), parse_csp(Cursor::new(f)));
    }
}

#[test]
fn nuke_vf() {
    for n in BAD_3D {
        let f = format!("#Inventor V2.1 ascii\ngrid_size {n} {n} {n}\ndata\n");
        rejected(&format!("vf {n}"), parse_vf(Cursor::new(f)));
    }
}

#[test]
fn pandora() {
    // `in` used to size `reserve` unchecked.
    for n in ["0", "1", "7", "9", "2146689", "4000000000000"] {
        let f = format!("channel: 3d\nin: {n}\nout: 256\nformat: lut\nvalues: red green blue\n");
        rejected(&format!("mga in {n}"), parse_mga(Cursor::new(f)));
    }
    // `out` 1 divided by zero.
    let mut f = String::from("channel: 3d\nin: 8\nout: 1\nformat: lut\nvalues: red green blue\n");
    for i in 0..8 {
        f.push_str(&format!("{i} 0 0 0\n"));
    }
    rejected("mga out 1", parse_mga(Cursor::new(f)));
}

#[test]
fn truelight() {
    for n in BAD_3D {
        let f = format!("# Truelight Cube v2.0\n# width {n} {n} {n}\n# Cube\n# end\n");
        rejected(&format!("cub {n}"), parse_cub(Cursor::new(f)));
    }
}

#[test]
fn discreet_1dl() {
    // A power of two with no upper bound used to size four `Vec::with_capacity`s.
    let f = format!("LUT: 1 {}\n0\n", 1usize << 40);
    rejected("1dl length 2^40", parse_1dl(Cursor::new(f)));
    // A non-ASCII data line used to panic slicing `[..4]` off a char boundary.
    let r = parse_1dl(Cursor::new("LUT: 1 256\nабв\n"));
    assert!(r.is_err());
}

#[test]
fn spi_and_3dl() {
    for n in BAD_3D {
        let f = format!("SPILUT 1.0\n3 3\n{n} {n} {n}\n");
        rejected(&format!("spi3d {n}"), parse_spi3d(Cursor::new(f)));
    }
    rejected(
        "spi1d Length 1",
        parse_spi1d(Cursor::new(
            "Version 1\nFrom 0 1\nLength 1\nComponents 1\n{\n0\n}\n",
        )),
    );
    // .3dl infers its size from the entry count: 2 entries is not a cube.
    rejected(
        "3dl 2 entries",
        parse_3dl(Cursor::new("0 512 1023\n0 0 0\n1023 1023 1023\n")),
    );
}
