(() => {
  "use strict";

  window.DemoApp.mount({
    id: "chronos",
    name: "Chronos",
    source: "crates/chronos",
    about:
      "Content-addressed commits, branches, 3-way merge. A core sample is history you can read layer by layer. Runs repository write, commit, branch, checkout, and log operations in memory.",
    help:
      "chronos init\n" +
      "chronos add PATH...\n" +
      "chronos commit -m MSG\n" +
      "chronos log\n" +
      "chronos branch [NAME]\n" +
      "chronos checkout TARGET\n" +
      "chronos cat-file ID",
    examples: [
      {
        label: "Branch & log (default)",
        input:
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
        label: "Linear history",
        input:
          "write a.txt v1\n" +
          "commit first\n" +
          "write a.txt v2\n" +
          "write b.txt sidecar\n" +
          "commit second\n" +
          "log"
      },
      {
        label: "Unknown branch (error)",
        input: "checkout nonexistent"
      }
    ]
  });
})();
