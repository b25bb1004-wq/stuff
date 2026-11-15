(() => {
  "use strict";

  window.DemoApp.mount({
    id: "hades",
    name: "Hades",
    source: "crates/hades",
    about:
      "Journaled, checksummed, extent-based. Hades splits into flat pages. Formats an in-memory block disk, runs directory and file operations, and checks consistency with fsck.",
    help:
      "hades mkfs IMG\n" +
      "hades ls IMG PATH\n" +
      "hades cat IMG PATH\n" +
      "hades put IMG SRC DST\n" +
      "hades rm IMG PATH\n" +
      "hades mkdir IMG PATH\n" +
      "hades fsck IMG",
    examples: [
      {
        label: "Notes lifecycle (default)",
        input:
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
        label: "Nested dirs & overwrite",
        input:
          "mkdir /var\n" +
          "mkdir /var/log\n" +
          "write /var/log/boot.txt stage1\n" +
          "write /var/log/boot.txt stage2 ready\n" +
          "cat /var/log/boot.txt\n" +
          "mkdir /var/tmp\n" +
          "rmdir /var/tmp\n" +
          "ls /var"
      },
      {
        label: "Missing file (error)",
        input: "cat /missing/file.txt"
      }
    ]
  });
})();
