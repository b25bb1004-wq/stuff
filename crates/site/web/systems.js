(function (global) {
  "use strict";

  global.MONOLITH_SYSTEMS = [
    {
      num: 1,
      id: "titan",
      name: "Titan",
      what: "Operating system",
      line: "Rust no_std kernel + userland on the Monolith CPU. The thick layer everything rides on.",
      source: "machine/titan",
      demo: "titan"
    },
    {
      num: 2,
      id: "iridium",
      name: "Iridium",
      what: "Database",
      line: "Paged B+tree, write-ahead log, SQL subset, server. A layer of sediment laid down in order.",
      source: "crates/iridium",
      demo: "iridium",
      defaultInput:
        "CREATE TABLE layers (id INT PRIMARY KEY, name TEXT, depth INT);\n" +
        "INSERT INTO layers VALUES (1, 'Titan', 100), (2, 'Iridium', 200), (3, 'Hades', 300);\n" +
        "SELECT name, depth FROM layers WHERE depth >= 150 ORDER BY depth DESC;\n" +
        "UPDATE layers SET depth = depth + 10 WHERE id = 1;\n" +
        "SELECT COUNT(*) AS total, SUM(depth) AS sum_depth FROM layers;"
    },
    {
      num: 3,
      id: "elysium",
      name: "Elysium",
      what: "Virtual machine",
      line: "Bytecode VM with garbage collector. A sealed world inside a plain rock.",
      source: "crates/elysium",
      demo: "elysium",
      defaultInput:
        "fn print(s: str) uses io = extern \"io.print\"\n\n" +
        "fn fact(n: int) -> int {\n" +
        "  if n <= 1 { 1 } else { n * fact(n - 1) }\n" +
        "}\n\n" +
        "fn main() uses io {\n" +
        "  let r = fact(6)\n" +
        "  print(\"6! = \" + str(r) + \"\\n\")\n" +
        "}"
    },
    {
      num: 4,
      id: "mercury",
      name: "Mercury",
      what: "Web server",
      line: "HTTP/1.1 server on our own thread pool. Where the rock breaks the surface and the world can see it.",
      source: "crates/mercury",
      demo: "mercury",
      defaultInput:
        "POST /api/layers HTTP/1.1\n" +
        "Host: monolith.local\n" +
        "Content-Type: text/plain\n" +
        "Content-Length: 13\n\n" +
        "hello mercury"
    },
    {
      num: 5,
      id: "helios",
      name: "Helios",
      what: "Game engine",
      line: "ECS, software renderer, physics, audio. The thing that moves worlds.",
      source: "crates/helios",
      demo: null
    },
    {
      num: 6,
      id: "cobalt",
      name: "Cobalt",
      what: "Compiler",
      line: "Logos to RV64 machine code and to bytecode. Sintering fuses loose powder into solid metal.",
      source: "crates/cobalt",
      demo: "cobalt",
      defaultInput:
        "fn print(s: str) uses io = extern \"io.print\"\n\n" +
        "fn main() uses io {\n" +
        "  let a = 2 + 3 * 4\n" +
        "  let b = a\n" +
        "  let unused = 99 - 1\n" +
        "  let sum = b + 28\n" +
        "  print(\"answer = \" + str(sum) + \"\\n\")\n" +
        "}"
    },
    {
      num: 7,
      id: "logos",
      name: "Logos",
      what: "Programming language",
      line: "Lexer, parser, type checker, IR. Logos splits into perfect thin layers, like its effect system.",
      source: "crates/logos",
      demo: "logos"
    },
    {
      num: 8,
      id: "odyssey",
      name: "Odyssey",
      what: "Browser",
      line: "HTML + CSS parser, layout, paint. Spelunking is exploring caves.",
      source: "crates/odyssey",
      demo: null
    },
    {
      num: 9,
      id: "aurum",
      name: "Aurum",
      what: "Blockchain",
      line: "Proof-of-stake chain, contracts on Elysium, stored in Iridium. One unbroken vein of ore.",
      source: "crates/aurum",
      demo: "aurum",
      defaultInput:
        "alice bob 100\n" +
        "alice carol 25\n" +
        "bob carol 40\n" +
        "carol alice 15"
    },
    {
      num: 10,
      id: "sphinx",
      name: "Sphinx + Medusa",
      what: "Encryption algorithm",
      line: "Sphinx: standard primitives proven on vectors. Medusa: our own experimental cipher, never the default.",
      source: "crates/sphinx",
      demo: "sphinx",
      defaultInput: "abc"
    },
    {
      num: 11,
      id: "silicon",
      name: "Silicon",
      what: "CPU emulator",
      line: "RV64 emulator with MMU, traps, devices. The crystal that keeps time for every real processor.",
      source: "crates/silicon",
      demo: "silicon",
      defaultInput:
        "    li t0, 0x10000000\n" +
        "    la t1, msg\n" +
        "loop:\n" +
        "    lbu t2, 0(t1)\n" +
        "    beqz t2, done\n" +
        "    sb t2, 0(t0)\n" +
        "    addi t1, t1, 1\n" +
        "    j loop\n" +
        "done:\n" +
        "    li a0, 42\n" +
        "    li a1, 100\n" +
        "    li a2, 0x1234\n" +
        "    li t0, 0x00100000\n" +
        "    li t1, 0x5555\n" +
        "    sw t1, 0(t0)\n" +
        "halt:\n" +
        "    j halt\n" +
        "msg:\n" +
        "    .asciz \"hello from silicon\\n\"",
      extraDemo: {
        id: "chisel",
        title: "Chisel RV64IMAC Assembler",
        source: "crates/chisel",
        defaultInput:
          "start:\n" +
          "    addi a0, zero, 42\n" +
          "    addi a1, zero, 13\n" +
          "    add  a2, a0, a1\n" +
          "    ebreak"
      }
    },
    {
      num: 12,
      id: "hades",
      name: "Hades",
      what: "File system",
      line: "Journaled, checksummed, extent-based. Hades splits into flat pages.",
      source: "crates/hades",
      demo: "hades",
      defaultInput:
        "mkdir /notes\n" +
        "write /notes/hello.txt Monolith Hades FS\n" +
        "ls /\n" +
        "ls /notes\n" +
        "cat /notes/hello.txt\n" +
        "write /notes/temp.txt scratch\n" +
        "rm /notes/temp.txt\n" +
        "ls /notes"
    },
    {
      num: 13,
      id: "pandora",
      name: "Pandora",
      what: "Container runtime",
      line: "Linux namespaces, cgroups v2, pivot_root. A small sealed cavity in rock.",
      source: "crates/pandora",
      demo: null
    },
    {
      num: 14,
      id: "argo",
      name: "Argo",
      what: "Package manager",
      line: "Signed packages, version solver, registry. A stack of stones that marks the trail.",
      source: "crates/argo",
      demo: "argo",
      defaultInput:
        "[index]\n" +
        "alpha 1.0.0 : beta ^1.0\n" +
        "alpha 1.1.0 : beta ^1.1\n" +
        "beta 1.0.0\n" +
        "beta 1.1.0 : gamma ^2.0\n" +
        "gamma 2.0.0\n" +
        "gamma 2.1.0\n\n" +
        "[manifest]\n" +
        "[package]\n" +
        "name = \"demo-app\"\n" +
        "version = \"0.1.0\"\n\n" +
        "[dependencies]\n" +
        "alpha = \"^1.0\"\n" +
        "gamma = \"2.0.*\""
    },
    {
      num: 15,
      id: "krypton",
      name: "Krypton",
      what: "Shell",
      line: "POSIX-style shell. The outermost shell of the planet.",
      source: "crates/krypton",
      demo: "krypton",
      defaultInput: "echo $X \"$HOME/notes\" $((X * 6)) ${UNSET:-fallback}"
    },
    {
      num: 16,
      id: "iris",
      name: "Iris",
      what: "Window manager",
      line: "Compositing window manager. Each window is one face of the cut stone.",
      source: "crates/iris",
      demo: null
    },
    {
      num: 17,
      id: "indium",
      name: "Indium",
      what: "GUI toolkit",
      line: "Widgets, layout, text, animation. What a polished surface gives back.",
      source: "crates/indium",
      demo: null
    },
    {
      num: 18,
      id: "calliope",
      name: "Calliope",
      what: "Text editor",
      line: "Rope-based modal editor. The first thing people wrote on.",
      source: "crates/calliope",
      demo: "calliope",
      defaultInput:
        "[A-Za-z_][A-Za-z0-9_]*\\([0-9]+\\)\n" +
        "fn main() {\n" +
        "  let a = calc(42)\n" +
        "  let b = step(7) + calc(108)\n" +
        "}"
    },
    {
      num: 19,
      id: "athena",
      name: "Athena",
      what: "IDE",
      line: "Browser IDE: editor + compiler + emulator + debugger. The workshop where stones are cut.",
      source: "web/athena",
      demo: "athena"
    },
    {
      num: 20,
      id: "chronos",
      name: "Chronos",
      what: "Version control",
      line: "Content-addressed commits, branches, 3-way merge. A core sample is history you can read layer by layer.",
      source: "crates/chronos",
      demo: "chronos",
      defaultInput:
        "write README.md Monolith Chronos VCS\n" +
        "write src/main.lg fn main() {}\n" +
        "commit initial commit\n" +
        "branch feat\n" +
        "checkout feat\n" +
        "write src/util.lg fn helper() -> int { 1 }\n" +
        "commit add helper\n" +
        "log\n" +
        "checkout main"
    },
    {
      num: 21,
      id: "xenon",
      name: "Xenon",
      what: "Network protocol",
      line: "Encrypted reliable transport over UDP. Signals that travel through the earth.",
      source: "crates/xenon",
      demo: null
    },
    {
      num: 22,
      id: "lithium",
      name: "Lithium",
      what: "Kernel in assembly",
      line: "A small kernel in pure RV64 assembly. Small, hard, makes the first spark.",
      source: "machine/lithium",
      demo: "lithium",
      defaultInput: "2000000"
    },
    {
      num: 23,
      id: "kairos",
      name: "Kairos",
      what: "Scheduler",
      line: "Fair scheduler core + work-stealing pool. Erupts on schedule.",
      source: "crates/kairos",
      demo: "kairos",
      defaultInput:
        "cpus 2\n" +
        "task alpha 4 12\n" +
        "task beta 2 12\n" +
        "task gamma 1 12"
    },
    {
      num: 24,
      id: "midas",
      name: "Midas",
      what: "Memory allocator",
      line: "Buddy + slab allocator. Where blocks are cut.",
      source: "crates/midas",
      demo: null
    },
    {
      num: 25,
      id: "zeus",
      name: "Zeus",
      what: "Hypervisor",
      line: "Type-1 hypervisor on the H extension. Nothing sits lower.",
      source: "machine/zeus",
      demo: null
    }
  ];
})(window);
