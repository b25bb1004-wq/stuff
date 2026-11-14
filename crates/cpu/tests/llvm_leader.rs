//! Leader cross-check: every word below was assembled by LLVM 19 (rustc global_asm, rv64im, no
//! compressed forms). Our decoder must read each as the instruction the assembler was given,
//! and our encoder must produce the same word. Generated; do not edit.
use strata_cpu::inst::{decode, encode, Inst::*};

const CASES: &[(u32, strata_cpu::inst::Inst)] = &[
    (
        0xa0c31537,
        Lui {
            rd: 10,
            imm: -1597829120,
        },
    ),
    (
        0x16564697,
        Auipc {
            rd: 13,
            imm: 374751232,
        },
    ),
    (0xe21f906f, Jal { rd: 0, imm: -25056 }),
    (
        0x29b88ae7,
        Jalr {
            rd: 21,
            rs1: 17,
            imm: 667,
        },
    ),
    (
        0xad600ae3,
        Beq {
            rs1: 0,
            rs2: 22,
            imm: -1324,
        },
    ),
    (
        0x2d821e63,
        Bne {
            rs1: 4,
            rs2: 24,
            imm: 732,
        },
    ),
    (
        0xb94a40e3,
        Blt {
            rs1: 20,
            rs2: 20,
            imm: -1152,
        },
    ),
    (
        0x5e1852e3,
        Bge {
            rs1: 16,
            rs2: 1,
            imm: 3556,
        },
    ),
    (
        0x7e9164e3,
        Bltu {
            rs1: 2,
            rs2: 9,
            imm: 4072,
        },
    ),
    (
        0xc2d97a63,
        Bgeu {
            rs1: 18,
            rs2: 13,
            imm: -3020,
        },
    ),
    (
        0x66660d83,
        Lb {
            rd: 27,
            rs1: 12,
            imm: 1638,
        },
    ),
    (
        0x0d4a1883,
        Lh {
            rd: 17,
            rs1: 20,
            imm: 212,
        },
    ),
    (
        0x8a37ac83,
        Lw {
            rd: 25,
            rs1: 15,
            imm: -1885,
        },
    ),
    (
        0xea3dbb03,
        Ld {
            rd: 22,
            rs1: 27,
            imm: -349,
        },
    ),
    (
        0x256f4083,
        Lbu {
            rd: 1,
            rs1: 30,
            imm: 598,
        },
    ),
    (
        0x6b08d503,
        Lhu {
            rd: 10,
            rs1: 17,
            imm: 1712,
        },
    ),
    (
        0x24c7ee83,
        Lwu {
            rd: 29,
            rs1: 15,
            imm: 588,
        },
    ),
    (
        0x8d648ca3,
        Sb {
            rs1: 9,
            rs2: 22,
            imm: -1831,
        },
    ),
    (
        0xfbff1c23,
        Sh {
            rs1: 30,
            rs2: 31,
            imm: -72,
        },
    ),
    (
        0x1c3425a3,
        Sw {
            rs1: 8,
            rs2: 3,
            imm: 459,
        },
    ),
    (
        0x4d42b5a3,
        Sd {
            rs1: 5,
            rs2: 20,
            imm: 1227,
        },
    ),
    (
        0x875b0713,
        Addi {
            rd: 14,
            rs1: 22,
            imm: -1931,
        },
    ),
    (
        0xc29c2913,
        Slti {
            rd: 18,
            rs1: 24,
            imm: -983,
        },
    ),
    (
        0xcd203e13,
        Sltiu {
            rd: 28,
            rs1: 0,
            imm: -814,
        },
    ),
    (
        0xe53e4593,
        Xori {
            rd: 11,
            rs1: 28,
            imm: -429,
        },
    ),
    (
        0x78ac6213,
        Ori {
            rd: 4,
            rs1: 24,
            imm: 1930,
        },
    ),
    (
        0x5954f813,
        Andi {
            rd: 16,
            rs1: 9,
            imm: 1429,
        },
    ),
    (
        0x3617009b,
        Addiw {
            rd: 1,
            rs1: 14,
            imm: 865,
        },
    ),
    (
        0x01151413,
        Slli {
            rd: 8,
            rs1: 10,
            imm: 17,
        },
    ),
    (
        0x00a5d513,
        Srli {
            rd: 10,
            rs1: 11,
            imm: 10,
        },
    ),
    (
        0x42f5d293,
        Srai {
            rd: 5,
            rs1: 11,
            imm: 47,
        },
    ),
    (
        0x0167979b,
        Slliw {
            rd: 15,
            rs1: 15,
            imm: 22,
        },
    ),
    (
        0x00e2da9b,
        Srliw {
            rd: 21,
            rs1: 5,
            imm: 14,
        },
    ),
    (
        0x4068581b,
        Sraiw {
            rd: 16,
            rs1: 16,
            imm: 6,
        },
    ),
    (
        0x001f03b3,
        Add {
            rd: 7,
            rs1: 30,
            rs2: 1,
        },
    ),
    (
        0x411a85b3,
        Sub {
            rd: 11,
            rs1: 21,
            rs2: 17,
        },
    ),
    (
        0x01141933,
        Sll {
            rd: 18,
            rs1: 8,
            rs2: 17,
        },
    ),
    (
        0x01332533,
        Slt {
            rd: 10,
            rs1: 6,
            rs2: 19,
        },
    ),
    (
        0x01be3f33,
        Sltu {
            rd: 30,
            rs1: 28,
            rs2: 27,
        },
    ),
    (
        0x0041c933,
        Xor {
            rd: 18,
            rs1: 3,
            rs2: 4,
        },
    ),
    (
        0x01315033,
        Srl {
            rd: 0,
            rs1: 2,
            rs2: 19,
        },
    ),
    (
        0x419658b3,
        Sra {
            rd: 17,
            rs1: 12,
            rs2: 25,
        },
    ),
    (
        0x0171e733,
        Or {
            rd: 14,
            rs1: 3,
            rs2: 23,
        },
    ),
    (
        0x0178f3b3,
        And {
            rd: 7,
            rs1: 17,
            rs2: 23,
        },
    ),
    (
        0x01588e3b,
        Addw {
            rd: 28,
            rs1: 17,
            rs2: 21,
        },
    ),
    (
        0x413d023b,
        Subw {
            rd: 4,
            rs1: 26,
            rs2: 19,
        },
    ),
    (
        0x01099e3b,
        Sllw {
            rd: 28,
            rs1: 19,
            rs2: 16,
        },
    ),
    (
        0x00aadfbb,
        Srlw {
            rd: 31,
            rs1: 21,
            rs2: 10,
        },
    ),
    (
        0x4015583b,
        Sraw {
            rd: 16,
            rs1: 10,
            rs2: 1,
        },
    ),
    (
        0x03ae0c33,
        Mul {
            rd: 24,
            rs1: 28,
            rs2: 26,
        },
    ),
    (
        0x03f39f33,
        Mulh {
            rd: 30,
            rs1: 7,
            rs2: 31,
        },
    ),
    (
        0x021428b3,
        Mulhsu {
            rd: 17,
            rs1: 8,
            rs2: 1,
        },
    ),
    (
        0x03f637b3,
        Mulhu {
            rd: 15,
            rs1: 12,
            rs2: 31,
        },
    ),
    (
        0x020bca33,
        Div {
            rd: 20,
            rs1: 23,
            rs2: 0,
        },
    ),
    (
        0x0333dfb3,
        Divu {
            rd: 31,
            rs1: 7,
            rs2: 19,
        },
    ),
    (
        0x02c76bb3,
        Rem {
            rd: 23,
            rs1: 14,
            rs2: 12,
        },
    ),
    (
        0x0238f333,
        Remu {
            rd: 6,
            rs1: 17,
            rs2: 3,
        },
    ),
    (
        0x02d88bbb,
        Mulw {
            rd: 23,
            rs1: 17,
            rs2: 13,
        },
    ),
    (
        0x02b84cbb,
        Divw {
            rd: 25,
            rs1: 16,
            rs2: 11,
        },
    ),
    (
        0x0281d4bb,
        Divuw {
            rd: 9,
            rs1: 3,
            rs2: 8,
        },
    ),
    (
        0x0241e13b,
        Remw {
            rd: 2,
            rs1: 3,
            rs2: 4,
        },
    ),
    (
        0x0297ff3b,
        Remuw {
            rd: 30,
            rs1: 15,
            rs2: 9,
        },
    ),
    (
        0x274fa4b7,
        Lui {
            rd: 9,
            imm: 659529728,
        },
    ),
    (
        0xab897717,
        Auipc {
            rd: 14,
            imm: -1417056256,
        },
    ),
    (
        0xef80f36f,
        Jal {
            rd: 6,
            imm: -985352,
        },
    ),
    (
        0x3d0d0ae7,
        Jalr {
            rd: 21,
            rs1: 26,
            imm: 976,
        },
    ),
    (
        0x6c6d08e3,
        Beq {
            rs1: 26,
            rs2: 6,
            imm: 3792,
        },
    ),
    (
        0x205a90e3,
        Bne {
            rs1: 21,
            rs2: 5,
            imm: 2560,
        },
    ),
    (
        0xcbddc063,
        Blt {
            rs1: 27,
            rs2: 29,
            imm: -2912,
        },
    ),
    (
        0x44a4d8e3,
        Bge {
            rs1: 9,
            rs2: 10,
            imm: 3152,
        },
    ),
    (
        0xa6136ae3,
        Bltu {
            rs1: 6,
            rs2: 1,
            imm: -1420,
        },
    ),
    (
        0x9148fae3,
        Bgeu {
            rs1: 17,
            rs2: 20,
            imm: -1772,
        },
    ),
    (
        0xf3f10a03,
        Lb {
            rd: 20,
            rs1: 2,
            imm: -193,
        },
    ),
    (
        0x0a569103,
        Lh {
            rd: 2,
            rs1: 13,
            imm: 165,
        },
    ),
    (
        0xcae1af03,
        Lw {
            rd: 30,
            rs1: 3,
            imm: -850,
        },
    ),
    (
        0x572a3b83,
        Ld {
            rd: 23,
            rs1: 20,
            imm: 1394,
        },
    ),
    (
        0xd4f0ce83,
        Lbu {
            rd: 29,
            rs1: 1,
            imm: -689,
        },
    ),
    (
        0x52b9dd03,
        Lhu {
            rd: 26,
            rs1: 19,
            imm: 1323,
        },
    ),
    (
        0x79e7e983,
        Lwu {
            rd: 19,
            rs1: 15,
            imm: 1950,
        },
    ),
    (
        0xec160fa3,
        Sb {
            rs1: 12,
            rs2: 1,
            imm: -289,
        },
    ),
    (
        0x2c0d9923,
        Sh {
            rs1: 27,
            rs2: 0,
            imm: 722,
        },
    ),
    (
        0xcb492aa3,
        Sw {
            rs1: 18,
            rs2: 20,
            imm: -843,
        },
    ),
    (
        0xf0593023,
        Sd {
            rs1: 18,
            rs2: 5,
            imm: -256,
        },
    ),
    (
        0xcf798013,
        Addi {
            rd: 0,
            rs1: 19,
            imm: -777,
        },
    ),
    (
        0x2d40a913,
        Slti {
            rd: 18,
            rs1: 1,
            imm: 724,
        },
    ),
    (
        0xb8083693,
        Sltiu {
            rd: 13,
            rs1: 16,
            imm: -1152,
        },
    ),
    (
        0xb431cb93,
        Xori {
            rd: 23,
            rs1: 3,
            imm: -1213,
        },
    ),
    (
        0x0f2c6793,
        Ori {
            rd: 15,
            rs1: 24,
            imm: 242,
        },
    ),
    (
        0xe482fc13,
        Andi {
            rd: 24,
            rs1: 5,
            imm: -440,
        },
    ),
    (
        0x6cbd8d9b,
        Addiw {
            rd: 27,
            rs1: 27,
            imm: 1739,
        },
    ),
    (
        0x01f19c13,
        Slli {
            rd: 24,
            rs1: 3,
            imm: 31,
        },
    ),
    (
        0x03e6d093,
        Srli {
            rd: 1,
            rs1: 13,
            imm: 62,
        },
    ),
    (
        0x40b5d413,
        Srai {
            rd: 8,
            rs1: 11,
            imm: 11,
        },
    ),
    (
        0x00a7189b,
        Slliw {
            rd: 17,
            rs1: 14,
            imm: 10,
        },
    ),
    (
        0x01c9551b,
        Srliw {
            rd: 10,
            rs1: 18,
            imm: 28,
        },
    ),
    (
        0x40ffde9b,
        Sraiw {
            rd: 29,
            rs1: 31,
            imm: 15,
        },
    ),
    (
        0x00d18c33,
        Add {
            rd: 24,
            rs1: 3,
            rs2: 13,
        },
    ),
    (
        0x40ba0fb3,
        Sub {
            rd: 31,
            rs1: 20,
            rs2: 11,
        },
    ),
    (
        0x00dc18b3,
        Sll {
            rd: 17,
            rs1: 24,
            rs2: 13,
        },
    ),
    (
        0x007f25b3,
        Slt {
            rd: 11,
            rs1: 30,
            rs2: 7,
        },
    ),
    (
        0x0056ba33,
        Sltu {
            rd: 20,
            rs1: 13,
            rs2: 5,
        },
    ),
    (
        0x0026cfb3,
        Xor {
            rd: 31,
            rs1: 13,
            rs2: 2,
        },
    ),
    (
        0x00d7d433,
        Srl {
            rd: 8,
            rs1: 15,
            rs2: 13,
        },
    ),
    (
        0x4104d933,
        Sra {
            rd: 18,
            rs1: 9,
            rs2: 16,
        },
    ),
    (
        0x01c06eb3,
        Or {
            rd: 29,
            rs1: 0,
            rs2: 28,
        },
    ),
    (
        0x01a8f633,
        And {
            rd: 12,
            rs1: 17,
            rs2: 26,
        },
    ),
    (
        0x00b00d3b,
        Addw {
            rd: 26,
            rs1: 0,
            rs2: 11,
        },
    ),
    (
        0x40b281bb,
        Subw {
            rd: 3,
            rs1: 5,
            rs2: 11,
        },
    ),
    (
        0x019f1e3b,
        Sllw {
            rd: 28,
            rs1: 30,
            rs2: 25,
        },
    ),
    (
        0x0115d23b,
        Srlw {
            rd: 4,
            rs1: 11,
            rs2: 17,
        },
    ),
    (
        0x40be56bb,
        Sraw {
            rd: 13,
            rs1: 28,
            rs2: 11,
        },
    ),
    (
        0x03018533,
        Mul {
            rd: 10,
            rs1: 3,
            rs2: 16,
        },
    ),
    (
        0x029c1b33,
        Mulh {
            rd: 22,
            rs1: 24,
            rs2: 9,
        },
    ),
    (
        0x0336ad33,
        Mulhsu {
            rd: 26,
            rs1: 13,
            rs2: 19,
        },
    ),
    (
        0x035fb333,
        Mulhu {
            rd: 6,
            rs1: 31,
            rs2: 21,
        },
    ),
    (
        0x034b4233,
        Div {
            rd: 4,
            rs1: 22,
            rs2: 20,
        },
    ),
    (
        0x0323d633,
        Divu {
            rd: 12,
            rs1: 7,
            rs2: 18,
        },
    ),
    (
        0x036364b3,
        Rem {
            rd: 9,
            rs1: 6,
            rs2: 22,
        },
    ),
    (
        0x0299fab3,
        Remu {
            rd: 21,
            rs1: 19,
            rs2: 9,
        },
    ),
    (
        0x03078ebb,
        Mulw {
            rd: 29,
            rs1: 15,
            rs2: 16,
        },
    ),
    (
        0x03cb423b,
        Divw {
            rd: 4,
            rs1: 22,
            rs2: 28,
        },
    ),
    (
        0x02375cbb,
        Divuw {
            rd: 25,
            rs1: 14,
            rs2: 3,
        },
    ),
    (
        0x03d96e3b,
        Remw {
            rd: 28,
            rs1: 18,
            rs2: 29,
        },
    ),
    (
        0x0267f03b,
        Remuw {
            rd: 0,
            rs1: 15,
            rs2: 6,
        },
    ),
    (
        0xc9352db7,
        Lui {
            rd: 27,
            imm: -919265280,
        },
    ),
    (
        0xd090e317,
        Auipc {
            rd: 6,
            imm: -795811840,
        },
    ),
    (0x1382b4ef, Jal { rd: 9, imm: 176440 }),
    (
        0xa8260fe7,
        Jalr {
            rd: 31,
            rs1: 12,
            imm: -1406,
        },
    ),
    (
        0x9a908263,
        Beq {
            rs1: 1,
            rs2: 9,
            imm: -3676,
        },
    ),
    (
        0x047898e3,
        Bne {
            rs1: 17,
            rs2: 7,
            imm: 2128,
        },
    ),
    (
        0x470bc4e3,
        Blt {
            rs1: 23,
            rs2: 16,
            imm: 3176,
        },
    ),
    (
        0x8bd65c63,
        Bge {
            rs1: 12,
            rs2: 29,
            imm: -3912,
        },
    ),
    (
        0x3d62ee63,
        Bltu {
            rs1: 5,
            rs2: 22,
            imm: 988,
        },
    ),
    (
        0x8f61f4e3,
        Bgeu {
            rs1: 3,
            rs2: 22,
            imm: -1816,
        },
    ),
    (
        0x2cc90e83,
        Lb {
            rd: 29,
            rs1: 18,
            imm: 716,
        },
    ),
    (
        0x0b731e03,
        Lh {
            rd: 28,
            rs1: 6,
            imm: 183,
        },
    ),
    (
        0x5dc8a503,
        Lw {
            rd: 10,
            rs1: 17,
            imm: 1500,
        },
    ),
    (
        0x6e203783,
        Ld {
            rd: 15,
            rs1: 0,
            imm: 1762,
        },
    ),
    (
        0xdd5d4c83,
        Lbu {
            rd: 25,
            rs1: 26,
            imm: -555,
        },
    ),
    (
        0x0c585303,
        Lhu {
            rd: 6,
            rs1: 16,
            imm: 197,
        },
    ),
    (
        0xc63c6583,
        Lwu {
            rd: 11,
            rs1: 24,
            imm: -925,
        },
    ),
    (
        0x53d108a3,
        Sb {
            rs1: 2,
            rs2: 29,
            imm: 1329,
        },
    ),
    (
        0x589b1f23,
        Sh {
            rs1: 22,
            rs2: 9,
            imm: 1438,
        },
    ),
    (
        0xd5f922a3,
        Sw {
            rs1: 18,
            rs2: 31,
            imm: -699,
        },
    ),
    (
        0xaad8baa3,
        Sd {
            rs1: 17,
            rs2: 13,
            imm: -1355,
        },
    ),
    (
        0x7c1e8b13,
        Addi {
            rd: 22,
            rs1: 29,
            imm: 1985,
        },
    ),
    (
        0x369a2093,
        Slti {
            rd: 1,
            rs1: 20,
            imm: 873,
        },
    ),
    (
        0x0b2fba93,
        Sltiu {
            rd: 21,
            rs1: 31,
            imm: 178,
        },
    ),
    (
        0xad144413,
        Xori {
            rd: 8,
            rs1: 8,
            imm: -1327,
        },
    ),
    (
        0xadd06b13,
        Ori {
            rd: 22,
            rs1: 0,
            imm: -1315,
        },
    ),
    (
        0xecc4f113,
        Andi {
            rd: 2,
            rs1: 9,
            imm: -308,
        },
    ),
    (
        0x58d2859b,
        Addiw {
            rd: 11,
            rs1: 5,
            imm: 1421,
        },
    ),
    (
        0x00d89a93,
        Slli {
            rd: 21,
            rs1: 17,
            imm: 13,
        },
    ),
    (
        0x032a5793,
        Srli {
            rd: 15,
            rs1: 20,
            imm: 50,
        },
    ),
    (
        0x41305293,
        Srai {
            rd: 5,
            rs1: 0,
            imm: 19,
        },
    ),
    (
        0x0167989b,
        Slliw {
            rd: 17,
            rs1: 15,
            imm: 22,
        },
    ),
    (
        0x01badb1b,
        Srliw {
            rd: 22,
            rs1: 21,
            imm: 27,
        },
    ),
    (
        0x415ed41b,
        Sraiw {
            rd: 8,
            rs1: 29,
            imm: 21,
        },
    ),
    (
        0x01718eb3,
        Add {
            rd: 29,
            rs1: 3,
            rs2: 23,
        },
    ),
    (
        0x40e18233,
        Sub {
            rd: 4,
            rs1: 3,
            rs2: 14,
        },
    ),
    (
        0x01af9cb3,
        Sll {
            rd: 25,
            rs1: 31,
            rs2: 26,
        },
    ),
    (
        0x01aca7b3,
        Slt {
            rd: 15,
            rs1: 25,
            rs2: 26,
        },
    ),
    (
        0x0086bfb3,
        Sltu {
            rd: 31,
            rs1: 13,
            rs2: 8,
        },
    ),
    (
        0x00d14433,
        Xor {
            rd: 8,
            rs1: 2,
            rs2: 13,
        },
    ),
    (
        0x00cad433,
        Srl {
            rd: 8,
            rs1: 21,
            rs2: 12,
        },
    ),
    (
        0x40d45933,
        Sra {
            rd: 18,
            rs1: 8,
            rs2: 13,
        },
    ),
    (
        0x0131ebb3,
        Or {
            rd: 23,
            rs1: 3,
            rs2: 19,
        },
    ),
    (
        0x0167f9b3,
        And {
            rd: 19,
            rs1: 15,
            rs2: 22,
        },
    ),
    (
        0x00fa00bb,
        Addw {
            rd: 1,
            rs1: 20,
            rs2: 15,
        },
    ),
    (
        0x40950c3b,
        Subw {
            rd: 24,
            rs1: 10,
            rs2: 9,
        },
    ),
    (
        0x00321f3b,
        Sllw {
            rd: 30,
            rs1: 4,
            rs2: 3,
        },
    ),
    (
        0x01fadfbb,
        Srlw {
            rd: 31,
            rs1: 21,
            rs2: 31,
        },
    ),
    (
        0x402d57bb,
        Sraw {
            rd: 15,
            rs1: 26,
            rs2: 2,
        },
    ),
    (
        0x03200333,
        Mul {
            rd: 6,
            rs1: 0,
            rs2: 18,
        },
    ),
    (
        0x02849d33,
        Mulh {
            rd: 26,
            rs1: 9,
            rs2: 8,
        },
    ),
    (
        0x03012eb3,
        Mulhsu {
            rd: 29,
            rs1: 2,
            rs2: 16,
        },
    ),
    (
        0x039e3833,
        Mulhu {
            rd: 16,
            rs1: 28,
            rs2: 25,
        },
    ),
    (
        0x0315c2b3,
        Div {
            rd: 5,
            rs1: 11,
            rs2: 17,
        },
    ),
    (
        0x03e7d833,
        Divu {
            rd: 16,
            rs1: 15,
            rs2: 30,
        },
    ),
    (
        0x03366f33,
        Rem {
            rd: 30,
            rs1: 12,
            rs2: 19,
        },
    ),
    (
        0x0341f6b3,
        Remu {
            rd: 13,
            rs1: 3,
            rs2: 20,
        },
    ),
    (
        0x033f82bb,
        Mulw {
            rd: 5,
            rs1: 31,
            rs2: 19,
        },
    ),
    (
        0x0307c9bb,
        Divw {
            rd: 19,
            rs1: 15,
            rs2: 16,
        },
    ),
    (
        0x02ad5b3b,
        Divuw {
            rd: 22,
            rs1: 26,
            rs2: 10,
        },
    ),
    (
        0x03d167bb,
        Remw {
            rd: 15,
            rs1: 2,
            rs2: 29,
        },
    ),
    (
        0x038c7b3b,
        Remuw {
            rd: 22,
            rs1: 24,
            rs2: 24,
        },
    ),
    (
        0x8e25eb37,
        Lui {
            rd: 22,
            imm: -1910120448,
        },
    ),
    (
        0x48842817,
        Auipc {
            rd: 16,
            imm: 1216618496,
        },
    ),
    (
        0xf3138aef,
        Jal {
            rd: 21,
            imm: -815312,
        },
    ),
    (
        0x04bf0f67,
        Jalr {
            rd: 30,
            rs1: 30,
            imm: 75,
        },
    ),
    (
        0xa83684e3,
        Beq {
            rs1: 13,
            rs2: 3,
            imm: -1400,
        },
    ),
    (
        0x6c6e9463,
        Bne {
            rs1: 29,
            rs2: 6,
            imm: 1736,
        },
    ),
    (
        0xc365c6e3,
        Blt {
            rs1: 11,
            rs2: 22,
            imm: -980,
        },
    ),
    (
        0x7fec5463,
        Bge {
            rs1: 24,
            rs2: 30,
            imm: 2024,
        },
    ),
    (
        0xaaaeece3,
        Bltu {
            rs1: 29,
            rs2: 10,
            imm: -1352,
        },
    ),
    (
        0x37b6fae3,
        Bgeu {
            rs1: 13,
            rs2: 27,
            imm: 2932,
        },
    ),
    (
        0xf8340b83,
        Lb {
            rd: 23,
            rs1: 8,
            imm: -125,
        },
    ),
    (
        0x5bed1e03,
        Lh {
            rd: 28,
            rs1: 26,
            imm: 1470,
        },
    ),
    (
        0xf717a583,
        Lw {
            rd: 11,
            rs1: 15,
            imm: -143,
        },
    ),
    (
        0xca65bc03,
        Ld {
            rd: 24,
            rs1: 11,
            imm: -858,
        },
    ),
    (
        0x677e4b83,
        Lbu {
            rd: 23,
            rs1: 28,
            imm: 1655,
        },
    ),
    (
        0x741f5b03,
        Lhu {
            rd: 22,
            rs1: 30,
            imm: 1857,
        },
    ),
    (
        0x5db06583,
        Lwu {
            rd: 11,
            rs1: 0,
            imm: 1499,
        },
    ),
    (
        0x07c38d23,
        Sb {
            rs1: 7,
            rs2: 28,
            imm: 122,
        },
    ),
    (
        0x92249523,
        Sh {
            rs1: 9,
            rs2: 2,
            imm: -1750,
        },
    ),
    (
        0x7b0527a3,
        Sw {
            rs1: 10,
            rs2: 16,
            imm: 1967,
        },
    ),
    (
        0x856f35a3,
        Sd {
            rs1: 30,
            rs2: 22,
            imm: -1973,
        },
    ),
    (
        0xe2788f13,
        Addi {
            rd: 30,
            rs1: 17,
            imm: -473,
        },
    ),
    (
        0x93a6a013,
        Slti {
            rd: 0,
            rs1: 13,
            imm: -1734,
        },
    ),
    (
        0xf8613a93,
        Sltiu {
            rd: 21,
            rs1: 2,
            imm: -122,
        },
    ),
    (
        0x5972ca93,
        Xori {
            rd: 21,
            rs1: 5,
            imm: 1431,
        },
    ),
    (
        0x1168e293,
        Ori {
            rd: 5,
            rs1: 17,
            imm: 278,
        },
    ),
    (
        0x58257093,
        Andi {
            rd: 1,
            rs1: 10,
            imm: 1410,
        },
    ),
    (
        0xd84e8f1b,
        Addiw {
            rd: 30,
            rs1: 29,
            imm: -636,
        },
    ),
    (
        0x03771213,
        Slli {
            rd: 4,
            rs1: 14,
            imm: 55,
        },
    ),
    (
        0x0314d093,
        Srli {
            rd: 1,
            rs1: 9,
            imm: 49,
        },
    ),
    (
        0x4386da93,
        Srai {
            rd: 21,
            rs1: 13,
            imm: 56,
        },
    ),
    (
        0x01e2931b,
        Slliw {
            rd: 6,
            rs1: 5,
            imm: 30,
        },
    ),
    (
        0x01b4529b,
        Srliw {
            rd: 5,
            rs1: 8,
            imm: 27,
        },
    ),
    (
        0x41be519b,
        Sraiw {
            rd: 3,
            rs1: 28,
            imm: 27,
        },
    ),
    (
        0x014e8f33,
        Add {
            rd: 30,
            rs1: 29,
            rs2: 20,
        },
    ),
    (
        0x40b18c33,
        Sub {
            rd: 24,
            rs1: 3,
            rs2: 11,
        },
    ),
    (
        0x01b099b3,
        Sll {
            rd: 19,
            rs1: 1,
            rs2: 27,
        },
    ),
    (
        0x01afabb3,
        Slt {
            rd: 23,
            rs1: 31,
            rs2: 26,
        },
    ),
    (
        0x002ab7b3,
        Sltu {
            rd: 15,
            rs1: 21,
            rs2: 2,
        },
    ),
    (
        0x01be4533,
        Xor {
            rd: 10,
            rs1: 28,
            rs2: 27,
        },
    ),
    (
        0x01b15f33,
        Srl {
            rd: 30,
            rs1: 2,
            rs2: 27,
        },
    ),
    (
        0x407b5cb3,
        Sra {
            rd: 25,
            rs1: 22,
            rs2: 7,
        },
    ),
    (
        0x011564b3,
        Or {
            rd: 9,
            rs1: 10,
            rs2: 17,
        },
    ),
    (
        0x00907bb3,
        And {
            rd: 23,
            rs1: 0,
            rs2: 9,
        },
    ),
    (
        0x01c88d3b,
        Addw {
            rd: 26,
            rs1: 17,
            rs2: 28,
        },
    ),
    (
        0x404582bb,
        Subw {
            rd: 5,
            rs1: 11,
            rs2: 4,
        },
    ),
    (
        0x003c943b,
        Sllw {
            rd: 8,
            rs1: 25,
            rs2: 3,
        },
    ),
    (
        0x01a95bbb,
        Srlw {
            rd: 23,
            rs1: 18,
            rs2: 26,
        },
    ),
    (
        0x41d3d43b,
        Sraw {
            rd: 8,
            rs1: 7,
            rs2: 29,
        },
    ),
    (
        0x03db83b3,
        Mul {
            rd: 7,
            rs1: 23,
            rs2: 29,
        },
    ),
    (
        0x020b9e33,
        Mulh {
            rd: 28,
            rs1: 23,
            rs2: 0,
        },
    ),
    (
        0x03b4ab33,
        Mulhsu {
            rd: 22,
            rs1: 9,
            rs2: 27,
        },
    ),
    (
        0x0249b5b3,
        Mulhu {
            rd: 11,
            rs1: 19,
            rs2: 4,
        },
    ),
    (
        0x02f6c033,
        Div {
            rd: 0,
            rs1: 13,
            rs2: 15,
        },
    ),
    (
        0x03d25fb3,
        Divu {
            rd: 31,
            rs1: 4,
            rs2: 29,
        },
    ),
    (
        0x021c67b3,
        Rem {
            rd: 15,
            rs1: 24,
            rs2: 1,
        },
    ),
    (
        0x03bff4b3,
        Remu {
            rd: 9,
            rs1: 31,
            rs2: 27,
        },
    ),
    (
        0x033e0d3b,
        Mulw {
            rd: 26,
            rs1: 28,
            rs2: 19,
        },
    ),
    (
        0x0377c83b,
        Divw {
            rd: 16,
            rs1: 15,
            rs2: 23,
        },
    ),
    (
        0x0383553b,
        Divuw {
            rd: 10,
            rs1: 6,
            rs2: 24,
        },
    ),
    (
        0x020860bb,
        Remw {
            rd: 1,
            rs1: 16,
            rs2: 0,
        },
    ),
    (
        0x0291703b,
        Remuw {
            rd: 0,
            rs1: 2,
            rs2: 9,
        },
    ),
    (
        0x59674eb7,
        Lui {
            rd: 29,
            imm: 1499938816,
        },
    ),
    (
        0x73e53997,
        Auipc {
            rd: 19,
            imm: 1944399872,
        },
    ),
    (0x2554d4ef, Jal { rd: 9, imm: 318036 }),
    (
        0x40db0467,
        Jalr {
            rd: 8,
            rs1: 22,
            imm: 1037,
        },
    ),
    (
        0x3b5300e3,
        Beq {
            rs1: 6,
            rs2: 21,
            imm: 2976,
        },
    ),
    (
        0xa0f994e3,
        Bne {
            rs1: 19,
            rs2: 15,
            imm: -1528,
        },
    ),
    (
        0x4089c8e3,
        Blt {
            rs1: 19,
            rs2: 8,
            imm: 3088,
        },
    ),
    (
        0xc6e45e63,
        Bge {
            rs1: 8,
            rs2: 14,
            imm: -2948,
        },
    ),
    (
        0x1567ece3,
        Bltu {
            rs1: 15,
            rs2: 22,
            imm: 2392,
        },
    ),
    (
        0xe562f863,
        Bgeu {
            rs1: 5,
            rs2: 22,
            imm: -2480,
        },
    ),
    (
        0xe2128c03,
        Lb {
            rd: 24,
            rs1: 5,
            imm: -479,
        },
    ),
    (
        0x2cf91983,
        Lh {
            rd: 19,
            rs1: 18,
            imm: 719,
        },
    ),
    (
        0xadfd2c83,
        Lw {
            rd: 25,
            rs1: 26,
            imm: -1313,
        },
    ),
    (
        0xbf713303,
        Ld {
            rd: 6,
            rs1: 2,
            imm: -1033,
        },
    ),
    (
        0x73a44003,
        Lbu {
            rd: 0,
            rs1: 8,
            imm: 1850,
        },
    ),
    (
        0x76ea5e03,
        Lhu {
            rd: 28,
            rs1: 20,
            imm: 1902,
        },
    ),
    (
        0x01c5ef03,
        Lwu {
            rd: 30,
            rs1: 11,
            imm: 28,
        },
    ),
    (
        0x036b04a3,
        Sb {
            rs1: 22,
            rs2: 22,
            imm: 41,
        },
    ),
    (
        0xe8f01ea3,
        Sh {
            rs1: 0,
            rs2: 15,
            imm: -355,
        },
    ),
    (
        0x59c12423,
        Sw {
            rs1: 2,
            rs2: 28,
            imm: 1416,
        },
    ),
    (
        0x823439a3,
        Sd {
            rs1: 8,
            rs2: 3,
            imm: -1997,
        },
    ),
    (
        0x3a158e13,
        Addi {
            rd: 28,
            rs1: 11,
            imm: 929,
        },
    ),
    (
        0xf63d2a93,
        Slti {
            rd: 21,
            rs1: 26,
            imm: -157,
        },
    ),
    (
        0xac67bb93,
        Sltiu {
            rd: 23,
            rs1: 15,
            imm: -1338,
        },
    ),
    (
        0xf4864093,
        Xori {
            rd: 1,
            rs1: 12,
            imm: -184,
        },
    ),
    (
        0x523e6993,
        Ori {
            rd: 19,
            rs1: 28,
            imm: 1315,
        },
    ),
    (
        0xda267a93,
        Andi {
            rd: 21,
            rs1: 12,
            imm: -606,
        },
    ),
    (
        0xaeba0d9b,
        Addiw {
            rd: 27,
            rs1: 20,
            imm: -1301,
        },
    ),
    (
        0x03841093,
        Slli {
            rd: 1,
            rs1: 8,
            imm: 56,
        },
    ),
    (
        0x007bdf13,
        Srli {
            rd: 30,
            rs1: 23,
            imm: 7,
        },
    ),
    (
        0x4008d393,
        Srai {
            rd: 7,
            rs1: 17,
            imm: 0,
        },
    ),
    (
        0x0008939b,
        Slliw {
            rd: 7,
            rs1: 17,
            imm: 0,
        },
    ),
    (
        0x00ce5a1b,
        Srliw {
            rd: 20,
            rs1: 28,
            imm: 12,
        },
    ),
    (
        0x4054db9b,
        Sraiw {
            rd: 23,
            rs1: 9,
            imm: 5,
        },
    ),
    (
        0x00bc0ab3,
        Add {
            rd: 21,
            rs1: 24,
            rs2: 11,
        },
    ),
    (
        0x41d80eb3,
        Sub {
            rd: 29,
            rs1: 16,
            rs2: 29,
        },
    ),
    (
        0x01451a33,
        Sll {
            rd: 20,
            rs1: 10,
            rs2: 20,
        },
    ),
    (
        0x0199a6b3,
        Slt {
            rd: 13,
            rs1: 19,
            rs2: 25,
        },
    ),
    (
        0x01bf3f33,
        Sltu {
            rd: 30,
            rs1: 30,
            rs2: 27,
        },
    ),
    (
        0x00a0cd33,
        Xor {
            rd: 26,
            rs1: 1,
            rs2: 10,
        },
    ),
    (
        0x017cdb33,
        Srl {
            rd: 22,
            rs1: 25,
            rs2: 23,
        },
    ),
    (
        0x41485233,
        Sra {
            rd: 4,
            rs1: 16,
            rs2: 20,
        },
    ),
    (
        0x011ae1b3,
        Or {
            rd: 3,
            rs1: 21,
            rs2: 17,
        },
    ),
    (
        0x00c872b3,
        And {
            rd: 5,
            rs1: 16,
            rs2: 12,
        },
    ),
    (
        0x010a86bb,
        Addw {
            rd: 13,
            rs1: 21,
            rs2: 16,
        },
    ),
    (
        0x40dc8b3b,
        Subw {
            rd: 22,
            rs1: 25,
            rs2: 13,
        },
    ),
    (
        0x00629b3b,
        Sllw {
            rd: 22,
            rs1: 5,
            rs2: 6,
        },
    ),
    (
        0x01e5dfbb,
        Srlw {
            rd: 31,
            rs1: 11,
            rs2: 30,
        },
    ),
    (
        0x40e3debb,
        Sraw {
            rd: 29,
            rs1: 7,
            rs2: 14,
        },
    ),
    (
        0x020504b3,
        Mul {
            rd: 9,
            rs1: 10,
            rs2: 0,
        },
    ),
    (
        0x025599b3,
        Mulh {
            rd: 19,
            rs1: 11,
            rs2: 5,
        },
    ),
    (
        0x029227b3,
        Mulhsu {
            rd: 15,
            rs1: 4,
            rs2: 9,
        },
    ),
    (
        0x03f537b3,
        Mulhu {
            rd: 15,
            rs1: 10,
            rs2: 31,
        },
    ),
    (
        0x037fc4b3,
        Div {
            rd: 9,
            rs1: 31,
            rs2: 23,
        },
    ),
    (
        0x03f4d6b3,
        Divu {
            rd: 13,
            rs1: 9,
            rs2: 31,
        },
    ),
    (
        0x039e6e33,
        Rem {
            rd: 28,
            rs1: 28,
            rs2: 25,
        },
    ),
    (
        0x0371f833,
        Remu {
            rd: 16,
            rs1: 3,
            rs2: 23,
        },
    ),
    (
        0x030c823b,
        Mulw {
            rd: 4,
            rs1: 25,
            rs2: 16,
        },
    ),
    (
        0x02e94f3b,
        Divw {
            rd: 30,
            rs1: 18,
            rs2: 14,
        },
    ),
    (
        0x0243d83b,
        Divuw {
            rd: 16,
            rs1: 7,
            rs2: 4,
        },
    ),
    (
        0x0361ed3b,
        Remw {
            rd: 26,
            rs1: 3,
            rs2: 22,
        },
    ),
    (
        0x025f7f3b,
        Remuw {
            rd: 30,
            rs1: 30,
            rs2: 5,
        },
    ),
    (
        0x772770b7,
        Lui {
            rd: 1,
            imm: 1999073280,
        },
    ),
    (
        0x5a512917,
        Auipc {
            rd: 18,
            imm: 1515266048,
        },
    ),
    (
        0xb75df86f,
        Jal {
            rd: 16,
            imm: -132236,
        },
    ),
    (
        0xa9360c67,
        Jalr {
            rd: 24,
            rs1: 12,
            imm: -1389,
        },
    ),
    (
        0x214a8063,
        Beq {
            rs1: 21,
            rs2: 20,
            imm: 512,
        },
    ),
    (
        0x580a1863,
        Bne {
            rs1: 20,
            rs2: 0,
            imm: 1424,
        },
    ),
    (
        0xb3f4ca63,
        Blt {
            rs1: 9,
            rs2: 31,
            imm: -3276,
        },
    ),
    (
        0xf53b5663,
        Bge {
            rs1: 22,
            rs2: 19,
            imm: -2228,
        },
    ),
    (
        0x12cbe6e3,
        Bltu {
            rs1: 23,
            rs2: 12,
            imm: 2348,
        },
    ),
    (
        0x6a7b78e3,
        Bgeu {
            rs1: 22,
            rs2: 7,
            imm: 3760,
        },
    ),
    (
        0x6fb78083,
        Lb {
            rd: 1,
            rs1: 15,
            imm: 1787,
        },
    ),
    (
        0x035b9d03,
        Lh {
            rd: 26,
            rs1: 23,
            imm: 53,
        },
    ),
    (
        0x47832d83,
        Lw {
            rd: 27,
            rs1: 6,
            imm: 1144,
        },
    ),
    (
        0x4011b483,
        Ld {
            rd: 9,
            rs1: 3,
            imm: 1025,
        },
    ),
    (
        0x6e574203,
        Lbu {
            rd: 4,
            rs1: 14,
            imm: 1765,
        },
    ),
    (
        0x55085003,
        Lhu {
            rd: 0,
            rs1: 16,
            imm: 1360,
        },
    ),
    (
        0x22b66703,
        Lwu {
            rd: 14,
            rs1: 12,
            imm: 555,
        },
    ),
    (
        0xf06d0223,
        Sb {
            rs1: 26,
            rs2: 6,
            imm: -252,
        },
    ),
    (
        0x20b81ea3,
        Sh {
            rs1: 16,
            rs2: 11,
            imm: 541,
        },
    ),
    (
        0x8adc2923,
        Sw {
            rs1: 24,
            rs2: 13,
            imm: -1870,
        },
    ),
    (
        0x0a19bea3,
        Sd {
            rs1: 19,
            rs2: 1,
            imm: 189,
        },
    ),
    (
        0x05b98a13,
        Addi {
            rd: 20,
            rs1: 19,
            imm: 91,
        },
    ),
    (
        0xc66f2913,
        Slti {
            rd: 18,
            rs1: 30,
            imm: -922,
        },
    ),
    (
        0x65f33c13,
        Sltiu {
            rd: 24,
            rs1: 6,
            imm: 1631,
        },
    ),
    (
        0x977acf13,
        Xori {
            rd: 30,
            rs1: 21,
            imm: -1673,
        },
    ),
    (
        0x10996193,
        Ori {
            rd: 3,
            rs1: 18,
            imm: 265,
        },
    ),
    (
        0xb4cffb13,
        Andi {
            rd: 22,
            rs1: 31,
            imm: -1204,
        },
    ),
    (
        0x311a0b9b,
        Addiw {
            rd: 23,
            rs1: 20,
            imm: 785,
        },
    ),
    (
        0x03f19f93,
        Slli {
            rd: 31,
            rs1: 3,
            imm: 63,
        },
    ),
    (
        0x03c25193,
        Srli {
            rd: 3,
            rs1: 4,
            imm: 60,
        },
    ),
    (
        0x43e4df13,
        Srai {
            rd: 30,
            rs1: 9,
            imm: 62,
        },
    ),
    (
        0x004a961b,
        Slliw {
            rd: 12,
            rs1: 21,
            imm: 4,
        },
    ),
    (
        0x0164dd1b,
        Srliw {
            rd: 26,
            rs1: 9,
            imm: 22,
        },
    ),
    (
        0x4192541b,
        Sraiw {
            rd: 8,
            rs1: 4,
            imm: 25,
        },
    ),
    (
        0x019e0db3,
        Add {
            rd: 27,
            rs1: 28,
            rs2: 25,
        },
    ),
    (
        0x41a50b33,
        Sub {
            rd: 22,
            rs1: 10,
            rs2: 26,
        },
    ),
    (
        0x017319b3,
        Sll {
            rd: 19,
            rs1: 6,
            rs2: 23,
        },
    ),
    (
        0x00122cb3,
        Slt {
            rd: 25,
            rs1: 4,
            rs2: 1,
        },
    ),
    (
        0x0022b633,
        Sltu {
            rd: 12,
            rs1: 5,
            rs2: 2,
        },
    ),
    (
        0x00b9c433,
        Xor {
            rd: 8,
            rs1: 19,
            rs2: 11,
        },
    ),
    (
        0x01dcd933,
        Srl {
            rd: 18,
            rs1: 25,
            rs2: 29,
        },
    ),
    (
        0x404759b3,
        Sra {
            rd: 19,
            rs1: 14,
            rs2: 4,
        },
    ),
    (
        0x0093e633,
        Or {
            rd: 12,
            rs1: 7,
            rs2: 9,
        },
    ),
    (
        0x000a7133,
        And {
            rd: 2,
            rs1: 20,
            rs2: 0,
        },
    ),
    (
        0x00090ebb,
        Addw {
            rd: 29,
            rs1: 18,
            rs2: 0,
        },
    ),
    (
        0x41c10cbb,
        Subw {
            rd: 25,
            rs1: 2,
            rs2: 28,
        },
    ),
    (
        0x01a21f3b,
        Sllw {
            rd: 30,
            rs1: 4,
            rs2: 26,
        },
    ),
    (
        0x002058bb,
        Srlw {
            rd: 17,
            rs1: 0,
            rs2: 2,
        },
    ),
    (
        0x4083d43b,
        Sraw {
            rd: 8,
            rs1: 7,
            rs2: 8,
        },
    ),
    (
        0x03a70f33,
        Mul {
            rd: 30,
            rs1: 14,
            rs2: 26,
        },
    ),
    (
        0x021499b3,
        Mulh {
            rd: 19,
            rs1: 9,
            rs2: 1,
        },
    ),
    (
        0x022420b3,
        Mulhsu {
            rd: 1,
            rs1: 8,
            rs2: 2,
        },
    ),
    (
        0x025eb5b3,
        Mulhu {
            rd: 11,
            rs1: 29,
            rs2: 5,
        },
    ),
    (
        0x0284c133,
        Div {
            rd: 2,
            rs1: 9,
            rs2: 8,
        },
    ),
    (
        0x02495e33,
        Divu {
            rd: 28,
            rs1: 18,
            rs2: 4,
        },
    ),
    (
        0x02c2e3b3,
        Rem {
            rd: 7,
            rs1: 5,
            rs2: 12,
        },
    ),
    (
        0x033c7633,
        Remu {
            rd: 12,
            rs1: 24,
            rs2: 19,
        },
    ),
    (
        0x02c1843b,
        Mulw {
            rd: 8,
            rs1: 3,
            rs2: 12,
        },
    ),
    (
        0x03b74fbb,
        Divw {
            rd: 31,
            rs1: 14,
            rs2: 27,
        },
    ),
    (
        0x0372da3b,
        Divuw {
            rd: 20,
            rs1: 5,
            rs2: 23,
        },
    ),
    (
        0x032ee53b,
        Remw {
            rd: 10,
            rs1: 29,
            rs2: 18,
        },
    ),
    (
        0x028bf6bb,
        Remuw {
            rd: 13,
            rs1: 23,
            rs2: 8,
        },
    ),
    (
        0x2460b2b7,
        Lui {
            rd: 5,
            imm: 610316288,
        },
    ),
    (
        0x933a4d17,
        Auipc {
            rd: 26,
            imm: -1824899072,
        },
    ),
    (
        0xd982956f,
        Jal {
            rd: 10,
            imm: -879208,
        },
    ),
    (
        0x40128b67,
        Jalr {
            rd: 22,
            rs1: 5,
            imm: 1025,
        },
    ),
    (
        0x56d50c63,
        Beq {
            rs1: 10,
            rs2: 13,
            imm: 1400,
        },
    ),
    (
        0xd04b9c63,
        Bne {
            rs1: 23,
            rs2: 4,
            imm: -2792,
        },
    ),
    (
        0xaca6c663,
        Blt {
            rs1: 13,
            rs2: 10,
            imm: -3380,
        },
    ),
    (
        0xf8d4d063,
        Bge {
            rs1: 9,
            rs2: 13,
            imm: -2176,
        },
    ),
    (
        0xee17ec63,
        Bltu {
            rs1: 15,
            rs2: 1,
            imm: -2312,
        },
    ),
    (
        0xe1fafa63,
        Bgeu {
            rs1: 21,
            rs2: 31,
            imm: -2540,
        },
    ),
    (
        0x65b70603,
        Lb {
            rd: 12,
            rs1: 14,
            imm: 1627,
        },
    ),
    (
        0xe5139203,
        Lh {
            rd: 4,
            rs1: 7,
            imm: -431,
        },
    ),
    (
        0xd700ac83,
        Lw {
            rd: 25,
            rs1: 1,
            imm: -656,
        },
    ),
    (
        0xc7503703,
        Ld {
            rd: 14,
            rs1: 0,
            imm: -907,
        },
    ),
    (
        0xe8a54783,
        Lbu {
            rd: 15,
            rs1: 10,
            imm: -374,
        },
    ),
    (
        0x22cf5f83,
        Lhu {
            rd: 31,
            rs1: 30,
            imm: 556,
        },
    ),
    (
        0x224f6a03,
        Lwu {
            rd: 20,
            rs1: 30,
            imm: 548,
        },
    ),
    (
        0xd05587a3,
        Sb {
            rs1: 11,
            rs2: 5,
            imm: -753,
        },
    ),
    (
        0x4e511ea3,
        Sh {
            rs1: 2,
            rs2: 5,
            imm: 1277,
        },
    ),
    (
        0x9fddac23,
        Sw {
            rs1: 27,
            rs2: 29,
            imm: -1544,
        },
    ),
    (
        0x56853923,
        Sd {
            rs1: 10,
            rs2: 8,
            imm: 1394,
        },
    ),
    (
        0x743d8b93,
        Addi {
            rd: 23,
            rs1: 27,
            imm: 1859,
        },
    ),
    (
        0xce05a713,
        Slti {
            rd: 14,
            rs1: 11,
            imm: -800,
        },
    ),
    (
        0x4c1f3013,
        Sltiu {
            rd: 0,
            rs1: 30,
            imm: 1217,
        },
    ),
    (
        0xce764293,
        Xori {
            rd: 5,
            rs1: 12,
            imm: -793,
        },
    ),
    (
        0xe71ce113,
        Ori {
            rd: 2,
            rs1: 25,
            imm: -399,
        },
    ),
    (
        0x5431fc93,
        Andi {
            rd: 25,
            rs1: 3,
            imm: 1347,
        },
    ),
    (
        0x20db0a1b,
        Addiw {
            rd: 20,
            rs1: 22,
            imm: 525,
        },
    ),
    (
        0x00569f93,
        Slli {
            rd: 31,
            rs1: 13,
            imm: 5,
        },
    ),
    (
        0x031bdf93,
        Srli {
            rd: 31,
            rs1: 23,
            imm: 49,
        },
    ),
    (
        0x40745c13,
        Srai {
            rd: 24,
            rs1: 8,
            imm: 7,
        },
    ),
    (
        0x01751d1b,
        Slliw {
            rd: 26,
            rs1: 10,
            imm: 23,
        },
    ),
    (
        0x015a5a1b,
        Srliw {
            rd: 20,
            rs1: 20,
            imm: 21,
        },
    ),
    (
        0x4006d11b,
        Sraiw {
            rd: 2,
            rs1: 13,
            imm: 0,
        },
    ),
    (
        0x019b8133,
        Add {
            rd: 2,
            rs1: 23,
            rs2: 25,
        },
    ),
    (
        0x41bf8f33,
        Sub {
            rd: 30,
            rs1: 31,
            rs2: 27,
        },
    ),
    (
        0x01f213b3,
        Sll {
            rd: 7,
            rs1: 4,
            rs2: 31,
        },
    ),
    (
        0x00fe23b3,
        Slt {
            rd: 7,
            rs1: 28,
            rs2: 15,
        },
    ),
    (
        0x016ab633,
        Sltu {
            rd: 12,
            rs1: 21,
            rs2: 22,
        },
    ),
    (
        0x00d4c3b3,
        Xor {
            rd: 7,
            rs1: 9,
            rs2: 13,
        },
    ),
    (
        0x0096d933,
        Srl {
            rd: 18,
            rs1: 13,
            rs2: 9,
        },
    ),
    (
        0x41f5deb3,
        Sra {
            rd: 29,
            rs1: 11,
            rs2: 31,
        },
    ),
    (
        0x01936433,
        Or {
            rd: 8,
            rs1: 6,
            rs2: 25,
        },
    ),
    (
        0x00a6ff33,
        And {
            rd: 30,
            rs1: 13,
            rs2: 10,
        },
    ),
    (
        0x016288bb,
        Addw {
            rd: 17,
            rs1: 5,
            rs2: 22,
        },
    ),
    (
        0x406388bb,
        Subw {
            rd: 17,
            rs1: 7,
            rs2: 6,
        },
    ),
    (
        0x00359a3b,
        Sllw {
            rd: 20,
            rs1: 11,
            rs2: 3,
        },
    ),
    (
        0x007ada3b,
        Srlw {
            rd: 20,
            rs1: 21,
            rs2: 7,
        },
    ),
    (
        0x41a25f3b,
        Sraw {
            rd: 30,
            rs1: 4,
            rs2: 26,
        },
    ),
    (
        0x02618cb3,
        Mul {
            rd: 25,
            rs1: 3,
            rs2: 6,
        },
    ),
    (
        0x02ab1c33,
        Mulh {
            rd: 24,
            rs1: 22,
            rs2: 10,
        },
    ),
    (
        0x039a2a33,
        Mulhsu {
            rd: 20,
            rs1: 20,
            rs2: 25,
        },
    ),
    (
        0x0301bcb3,
        Mulhu {
            rd: 25,
            rs1: 3,
            rs2: 16,
        },
    ),
    (
        0x026cc933,
        Div {
            rd: 18,
            rs1: 25,
            rs2: 6,
        },
    ),
    (
        0x02e45033,
        Divu {
            rd: 0,
            rs1: 8,
            rs2: 14,
        },
    ),
    (
        0x0283e233,
        Rem {
            rd: 4,
            rs1: 7,
            rs2: 8,
        },
    ),
    (
        0x02297b33,
        Remu {
            rd: 22,
            rs1: 18,
            rs2: 2,
        },
    ),
    (
        0x03790abb,
        Mulw {
            rd: 21,
            rs1: 18,
            rs2: 23,
        },
    ),
    (
        0x0308c43b,
        Divw {
            rd: 8,
            rs1: 17,
            rs2: 16,
        },
    ),
    (
        0x022c5a3b,
        Divuw {
            rd: 20,
            rs1: 24,
            rs2: 2,
        },
    ),
    (
        0x02a7ef3b,
        Remw {
            rd: 30,
            rs1: 15,
            rs2: 10,
        },
    ),
    (
        0x0399733b,
        Remuw {
            rd: 6,
            rs1: 18,
            rs2: 25,
        },
    ),
    (
        0xe4e71eb7,
        Lui {
            rd: 29,
            imm: -454619136,
        },
    ),
    (
        0x8ba0d897,
        Auipc {
            rd: 17,
            imm: -1952395264,
        },
    ),
    (
        0xb39a386f,
        Jal {
            rd: 16,
            imm: -378056,
        },
    ),
    (
        0x0cae0167,
        Jalr {
            rd: 2,
            rs1: 28,
            imm: 202,
        },
    ),
    (
        0xe52b06e3,
        Beq {
            rs1: 22,
            rs2: 18,
            imm: -436,
        },
    ),
    (
        0x8d6b1863,
        Bne {
            rs1: 22,
            rs2: 22,
            imm: -3888,
        },
    ),
    (
        0xabbe46e3,
        Blt {
            rs1: 28,
            rs2: 27,
            imm: -1364,
        },
    ),
    (
        0x604dd463,
        Bge {
            rs1: 27,
            rs2: 4,
            imm: 1544,
        },
    ),
    (
        0x9c396063,
        Bltu {
            rs1: 18,
            rs2: 3,
            imm: -3648,
        },
    ),
    (
        0xfb2ffce3,
        Bgeu {
            rs1: 31,
            rs2: 18,
            imm: -72,
        },
    ),
    (
        0x12500f83,
        Lb {
            rd: 31,
            rs1: 0,
            imm: 293,
        },
    ),
    (
        0xeff61f83,
        Lh {
            rd: 31,
            rs1: 12,
            imm: -257,
        },
    ),
    (
        0x6ba62a03,
        Lw {
            rd: 20,
            rs1: 12,
            imm: 1722,
        },
    ),
    (
        0xbd3e3803,
        Ld {
            rd: 16,
            rs1: 28,
            imm: -1069,
        },
    ),
    (
        0x0f1ac283,
        Lbu {
            rd: 5,
            rs1: 21,
            imm: 241,
        },
    ),
    (
        0xfd845283,
        Lhu {
            rd: 5,
            rs1: 8,
            imm: -40,
        },
    ),
    (
        0xad83e683,
        Lwu {
            rd: 13,
            rs1: 7,
            imm: -1320,
        },
    ),
    (
        0x43d20ba3,
        Sb {
            rs1: 4,
            rs2: 29,
            imm: 1079,
        },
    ),
    (
        0x905b1a23,
        Sh {
            rs1: 22,
            rs2: 5,
            imm: -1772,
        },
    ),
    (
        0xf11aaba3,
        Sw {
            rs1: 21,
            rs2: 17,
            imm: -233,
        },
    ),
    (
        0xec9f3aa3,
        Sd {
            rs1: 30,
            rs2: 9,
            imm: -299,
        },
    ),
    (
        0x69e60293,
        Addi {
            rd: 5,
            rs1: 12,
            imm: 1694,
        },
    ),
    (
        0x77b12d13,
        Slti {
            rd: 26,
            rs1: 2,
            imm: 1915,
        },
    ),
    (
        0x70a4b493,
        Sltiu {
            rd: 9,
            rs1: 9,
            imm: 1802,
        },
    ),
    (
        0xd6cc4313,
        Xori {
            rd: 6,
            rs1: 24,
            imm: -660,
        },
    ),
    (
        0xe1f0eb13,
        Ori {
            rd: 22,
            rs1: 1,
            imm: -481,
        },
    ),
    (
        0xccc87a93,
        Andi {
            rd: 21,
            rs1: 16,
            imm: -820,
        },
    ),
    (
        0x230a031b,
        Addiw {
            rd: 6,
            rs1: 20,
            imm: 560,
        },
    ),
    (
        0x02ce1993,
        Slli {
            rd: 19,
            rs1: 28,
            imm: 44,
        },
    ),
    (
        0x00b9da93,
        Srli {
            rd: 21,
            rs1: 19,
            imm: 11,
        },
    ),
    (
        0x40ac5613,
        Srai {
            rd: 12,
            rs1: 24,
            imm: 10,
        },
    ),
    (
        0x0065109b,
        Slliw {
            rd: 1,
            rs1: 10,
            imm: 6,
        },
    ),
    (
        0x0109dc1b,
        Srliw {
            rd: 24,
            rs1: 19,
            imm: 16,
        },
    ),
    (
        0x40bb5c1b,
        Sraiw {
            rd: 24,
            rs1: 22,
            imm: 11,
        },
    ),
    (
        0x01c58e33,
        Add {
            rd: 28,
            rs1: 11,
            rs2: 28,
        },
    ),
    (
        0x41c18b33,
        Sub {
            rd: 22,
            rs1: 3,
            rs2: 28,
        },
    ),
    (
        0x010b13b3,
        Sll {
            rd: 7,
            rs1: 22,
            rs2: 16,
        },
    ),
    (
        0x0014a1b3,
        Slt {
            rd: 3,
            rs1: 9,
            rs2: 1,
        },
    ),
    (
        0x007cbcb3,
        Sltu {
            rd: 25,
            rs1: 25,
            rs2: 7,
        },
    ),
    (
        0x01e94eb3,
        Xor {
            rd: 29,
            rs1: 18,
            rs2: 30,
        },
    ),
    (
        0x009358b3,
        Srl {
            rd: 17,
            rs1: 6,
            rs2: 9,
        },
    ),
    (
        0x40a75033,
        Sra {
            rd: 0,
            rs1: 14,
            rs2: 10,
        },
    ),
    (
        0x0090e633,
        Or {
            rd: 12,
            rs1: 1,
            rs2: 9,
        },
    ),
    (
        0x0129fc33,
        And {
            rd: 24,
            rs1: 19,
            rs2: 18,
        },
    ),
    (
        0x0098873b,
        Addw {
            rd: 14,
            rs1: 17,
            rs2: 9,
        },
    ),
    (
        0x41d00abb,
        Subw {
            rd: 21,
            rs1: 0,
            rs2: 29,
        },
    ),
    (
        0x00f81fbb,
        Sllw {
            rd: 31,
            rs1: 16,
            rs2: 15,
        },
    ),
    (
        0x009ed4bb,
        Srlw {
            rd: 9,
            rs1: 29,
            rs2: 9,
        },
    ),
    (
        0x409fd7bb,
        Sraw {
            rd: 15,
            rs1: 31,
            rs2: 9,
        },
    ),
    (
        0x02ab09b3,
        Mul {
            rd: 19,
            rs1: 22,
            rs2: 10,
        },
    ),
    (
        0x02ef1f33,
        Mulh {
            rd: 30,
            rs1: 30,
            rs2: 14,
        },
    ),
    (
        0x034d22b3,
        Mulhsu {
            rd: 5,
            rs1: 26,
            rs2: 20,
        },
    ),
    (
        0x02af36b3,
        Mulhu {
            rd: 13,
            rs1: 30,
            rs2: 10,
        },
    ),
    (
        0x0325cb33,
        Div {
            rd: 22,
            rs1: 11,
            rs2: 18,
        },
    ),
    (
        0x0312d333,
        Divu {
            rd: 6,
            rs1: 5,
            rs2: 17,
        },
    ),
    (
        0x03906f33,
        Rem {
            rd: 30,
            rs1: 0,
            rs2: 25,
        },
    ),
    (
        0x0271f933,
        Remu {
            rd: 18,
            rs1: 3,
            rs2: 7,
        },
    ),
    (
        0x03118e3b,
        Mulw {
            rd: 28,
            rs1: 3,
            rs2: 17,
        },
    ),
    (
        0x0395c33b,
        Divw {
            rd: 6,
            rs1: 11,
            rs2: 25,
        },
    ),
    (
        0x02ed5c3b,
        Divuw {
            rd: 24,
            rs1: 26,
            rs2: 14,
        },
    ),
    (
        0x03fae03b,
        Remw {
            rd: 0,
            rs1: 21,
            rs2: 31,
        },
    ),
    (
        0x02eaf23b,
        Remuw {
            rd: 4,
            rs1: 21,
            rs2: 14,
        },
    ),
    (0x00000073, Ecall),
    (0x00100073, Ebreak),
    (0x0ff0000f, Fence),
];

#[test]
fn decoder_and_encoder_agree_with_llvm() {
    for (word, inst) in CASES {
        assert_eq!(decode(*word), Some(*inst), "decode {word:#010x}");
        assert_eq!(encode(*inst), *word, "encode {inst:?}");
    }
}
