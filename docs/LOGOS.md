# Logos: the Monolith language (version 1)

Logos is a small, statically typed, deterministic language. One program compiles to two targets:
Logos bytecode for the Monolith VM (`docs/MBC.md`) and RV64 machine code for Monolith OS. Both must
print the same output for the same input.

Three things make it Logos and not a copy of something else:
1. **Layers.** A function is sealed from the outside world unless its signature says which
   layers it reaches: `fn save(p: Post) uses fs, net`. The compiler proves it. A smart contract
   is a program whose entry point uses only `chain`, so it is deterministic by type, and
   `logos run --deny net` refuses to start a program that could touch the network.
2. **Tests and properties are syntax.** `test "..." { }` and `prop "..." (x: int) { }` live next
   to the code and run with the same seeded generator on every machine.
3. **No undefined behaviour.** Integer overflow, division by zero, bad indexes and failed
   asserts are traps with a source position. There is no null, no uninitialised value.

## Lexical rules
- Source is UTF-8. Comments: `//` to end of line. No block comments.
- Names: `lower_case` for values, functions and layers (`[a-z_][a-zA-Z0-9_]*`); `UpperCase` for
  types and variants (`[A-Z][a-zA-Z0-9]*`). The lexer emits them as two token kinds, `LIdent`
  and `UIdent`. This is what lets `if ok { .. }` and `Point { x: 1 }` be told apart.
- Keywords: `fn type layer const let var if else match while for in return break continue
  assert test prop where uses extern true false none some`.
- Integers: decimal `1_000`, hex `0xff`, binary `0b1010`. Value must fit `int` (i64) or it is a
  lex error. Byte literal: `b'a'`, with escapes `\n \t \r \0 \\ \' \xNN`.
- Strings: `"..."` with escapes `\n \t \r \0 \\ \" \xNN \u{1F600}`. No raw or multi-line strings.
- Operators and punctuation:
  `+ - * / % +% -% *% == != < <= > >= && || ! & | ^ << >> = |> -> => . .. , : ; ? ( ) [ ] { }`
- **Statement ends.** The lexer emits a `Newline` token at a line break, except when:
  (a) the last token on the line is an operator, `,`, `:`, `->`, `=>`, `=` or an opening bracket;
  (b) the first token on the next line is `|>`, `.`, `&&`, `||`, `else`, or a closing bracket;
  (c) the break is inside `( )` or `[ ]`.
  A `;` is the same as `Newline`. Several `Newline` tokens in a row count as one.

## Grammar (EBNF; `NL` is the Newline token, skipped freely between items and after `{`)
```
module   = { item NL } ;
item     = typedef | constdef | layerdef | fndef | testdef | propdef ;
typedef  = "type" UIdent "=" ( record | variant { "|" variant } ) ;
record   = "{" field { "," field } [ "," ] "}" ;
field    = LIdent ":" type ;
variant  = UIdent [ "(" type { "," type } ")" ] ;
constdef = "const" LIdent ":" type "=" expr ;
layerdef = "layer" LIdent ;
fndef    = "fn" LIdent "(" [ param { "," param } ] ")" [ "->" type ] [ uses ]
           ( block | "=" "extern" String ) ;
param    = LIdent ":" type ;
uses     = "uses" LIdent { "," LIdent } ;
testdef  = "test" String block ;
propdef  = "prop" String "(" param { "," param } ")" [ "where" expr ] block ;

type     = basetype { "?" } ;
basetype = "int" | "bool" | "byte" | "str" | "unit" | UIdent
         | "[" type "]"
         | "(" type "," type { "," type } ")"
         | "fn" "(" [ type { "," type } ] ")" "->" type [ uses ] ;

block    = "{" { stmt NL } "}" ;
stmt     = "let" LIdent [ ":" type ] "=" expr
         | "var" LIdent [ ":" type ] "=" expr
         | "return" [ expr ] | "break" | "continue"
         | "while" expr block
         | "for" LIdent "in" expr block
         | "assert" expr [ "," expr ]
         | expr [ "=" expr ] ;                  (* with "=", the left side must be a place *)

expr     = pipe ;
pipe     = or { "|>" postfix } ;                (* x |> f(a, b)  means  f(x, a, b) *)
or       = and { "||" and } ;
and      = cmp { "&&" cmp } ;
cmp      = range [ ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) range ] ;   (* not chainable *)
range    = bitor [ ".." bitor ] ;
bitor    = bitxor { "|" bitxor } ;
bitxor   = bitand { "^" bitand } ;
bitand   = shift { "&" shift } ;
shift    = sum { ( "<<" | ">>" ) sum } ;
sum      = product { ( "+" | "-" | "+%" | "-%" ) product } ;
product  = unary { ( "*" | "/" | "%" | "*%" ) unary } ;
unary    = ( "-" | "!" ) unary | postfix ;
postfix  = primary { "(" [ args ] ")" | "[" expr "]" | "." LIdent | "." Int } ;
args     = expr { "," expr } [ "," ] ;
primary  = Int | Byte | String | "true" | "false" | "none" | "some" "(" expr ")"
         | LIdent
         | UIdent [ "(" args ")" ]                          (* variant value *)
         | UIdent "{" LIdent ":" expr { "," LIdent ":" expr } [ "," ] "}"   (* record value *)
         | "[" [ args ] "]" | "[" expr ";" expr "]"         (* list, or value repeated n times *)
         | "(" expr ")" | "(" expr "," expr { "," expr } ")" | "(" ")"   (* group, tuple, unit *)
         | "if" expr block [ "else" ( block | ifexpr ) ]
         | "match" expr "{" arm { "," arm } [ "," ] "}"
         | block ;
arm      = pattern "=>" expr ;
pattern  = "_" | LIdent | Int | Byte | String | "true" | "false" | "none" | "some" "(" pattern ")"
         | UIdent [ "(" pattern { "," pattern } ")" ]
         | "(" pattern "," pattern { "," pattern } ")" ;
```
In the condition of `if`, `while`, `for .. in` and the subject of `match`, a record value must be
wrapped in parentheses.

## Types
| Type | Meaning |
|---|---|
| `int` | 64-bit signed integer |
| `byte` | 8-bit unsigned integer |
| `bool`, `unit` | `true`/`false`; the single value `()` |
| `str` | immutable UTF-8 text |
| `[T]` | growable array, a shared mutable object |
| `(A, B, ..)` | tuple, fields `.0`, `.1` |
| `T?` | optional: `none` or `some(v)` |
| `Name` | a record (`type P = { x: int }`, shared mutable object) or a variant type |
| `fn(A) -> B uses l` | function value; only named top-level functions, no closures in version 1 |

- No implicit conversions. `int(b)` widens a byte; `byte(i)` traps when out of range.
- Function signatures are always written. `let`/`var` types are inferred from the right side.
  `[]` and `none` need a known type from an annotation or the surrounding expression.
- `==` and `!=` work on `int byte bool str unit` and on tuples and options of those. `<` etc.
  work on `int byte str`.
- A block's value is the value of its last statement when that is an expression; otherwise
  `unit`. `if` without `else` has type `unit`. Both `if` branches and all `match` arms must have
  the same type. A function with no `-> T` returns `unit`.
- `match` must be exhaustive: all variants, both bools, `none` and `some`, or a `_`/name arm.
- `let` binds once. `var` can be assigned. Places: a `var` name, `place.field`, `place[index]`.
- `for x in a..b` counts from `a` up to but not including `b`. `for x in xs` walks an array by
  index; the length is read again each step. Ranges exist only in `for` and in slices `xs[a..b]`.

## Layers (the effect system)
- Built-in layers: `io fs net time rand chain sys`. `layer name` declares another.
- Every function has a layer set, the names after `uses` (empty when absent).
- Rule: for every call inside a function body, the callee's set must be a subset of the caller's.
  Calling through a function value uses the set written in its type. A function value may be
  passed where its set is a subset of the expected type's set.
- `extern` functions are the only source of layers: `fn print(s: str) uses io = extern "io.print"`.
- `prop` bodies must have an empty set (so every failure can be replayed). `test` bodies may use
  any layer. `const` initialisers are literals or constant arithmetic.
- The checker's error for a violation names the full call path:
  `layer 'net' is not allowed here: main -> sync -> fetch (uses net)`.
- Mutating arrays and records is not a layer. Layers describe the world outside the program.

## Semantics
- Evaluation is strictly left to right; arguments before the call. `&&` and `||` short-circuit.
- `+ - *` trap on overflow (`Overflow`). `+% -% *%` wrap. Unary `-` traps on the minimum int.
- `/` and `%` trap on a zero divisor (`DivideByZero`) and on `MIN / -1`. They truncate toward zero.
- `<<` and `>>` trap when the count is not in `0..=63`. `>>` is arithmetic.
- Indexing and slicing trap when out of range (`IndexOutOfRange`). `assert` traps with
  `AssertFailed` and the optional message.
- Byte arithmetic uses the same operators and traps outside `0..=255`.
- Traps stop the program. There are no exceptions; use `T?` and variant types for errors.
- The entry point is `fn main()` with any layer set and no parameters.

## Built-in functions (no layers)
`len(x)` for `str` (bytes) and arrays; `push(xs, v)`; `pop(xs) -> T?`; `xs[a..b]` copies a slice;
`a + b` joins two `str` or two arrays; `str(int) -> str`; `parse_int(str) -> int?`;
`bytes(str) -> [byte]`; `text([byte]) -> str?` (none when not UTF-8); `int(byte) -> int`;
`byte(int) -> byte`; `min(a, b)`, `max(a, b)`, `abs(a)` on `int`.

## Standard externs (provided by the host; this list is the whole version 1 world)
| Signature | Import name |
|---|---|
| `print(s: str) uses io` | `io.print` |
| `read_line() -> str? uses io` | `io.read_line` |
| `now() -> int uses time` (milliseconds) | `time.now` |
| `random() -> int uses rand` | `rand.next` |
| `read_file(path: str) -> [byte]? uses fs` | `fs.read` |
| `write_file(path: str, data: [byte]) -> bool uses fs` | `fs.write` |
| `storage_get(key: [byte]) -> [byte]? uses chain` | `chain.get` |
| `storage_set(key: [byte], value: [byte]) uses chain` | `chain.set` |
| `caller() -> [byte] uses chain` | `chain.caller` |
| `emit(event: str) uses chain` | `chain.emit` |

## Example
```
type Shape = Circle(int) | Rect(int, int)

fn area(s: Shape) -> int {
    match s {
        Circle(r) => 3 * r * r,
        Rect(w, h) => w * h,
    }
}

fn total(shapes: [Shape]) -> int {
    var sum = 0
    for s in shapes { sum = sum + area(s) }
    sum
}

fn main() uses io {
    [Circle(2), Rect(3, 4)] |> total() |> str() |> print()
}

test "a rectangle" { assert area(Rect(3, 4)) == 12 }

prop "area is never negative" (w: int, h: int) where w >= 0 && w < 1000 && h >= 0 && h < 1000 {
    assert area(Rect(w, h)) >= 0
}
```

## Not in version 1 (reserved, so nobody builds them by accident)
Closures, user generics, modules and `use`, floating point, traits or interfaces, string
formatting syntax, concurrency.
