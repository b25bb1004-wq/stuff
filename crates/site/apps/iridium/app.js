(() => {
  "use strict";

  window.DemoApp.mount({
    id: "iridium",
    name: "Iridium",
    source: "crates/iridium",
    about:
      "Paged B+tree, write-ahead log, SQL subset, server. A layer of sediment laid down in order. Executes multi-statement SQL scripts in an in-memory transactional engine.",
    help:
      "iridium sql DB.idb [QUERY or -]\n" +
      "iridium connect ADDR\n" +
      "Also ships iridium-cli for interactive sessions.",
    examples: [
      {
        label: "Layers table (default)",
        input:
          "CREATE TABLE layers (id INT PRIMARY KEY, name TEXT, depth INT);\n" +
          "INSERT INTO layers VALUES (1, 'Titan', 100), (2, 'Iridium', 200), (3, 'Hades', 300);\n" +
          "SELECT name, depth FROM layers WHERE depth >= 150 ORDER BY depth DESC;\n" +
          "UPDATE layers SET depth = depth + 10 WHERE id = 1;\n" +
          "SELECT COUNT(*) AS total, SUM(depth) AS sum_depth FROM layers;"
      },
      {
        label: "Inventory filter & delete",
        input:
          "CREATE TABLE rocks (id INT PRIMARY KEY, kind TEXT, mass INT);\n" +
          "INSERT INTO rocks VALUES (1, 'basalt', 40), (2, 'granite', 65), (3, 'shale', 25), (4, 'slate', 50);\n" +
          "DELETE FROM rocks WHERE mass < 30;\n" +
          "SELECT id, kind, mass FROM rocks WHERE mass >= 40 ORDER BY mass ASC;"
      },
      {
        label: "Missing table (error)",
        input: "SELECT * FROM missing_table;"
      }
    ]
  });
})();
