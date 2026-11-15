(() => {
  "use strict";

  window.DemoApp.mount({
    id: "kairos",
    name: "Kairos",
    source: "crates/kairos",
    about:
      "Fair scheduler core + work-stealing pool. Erupts on schedule. Simulates weighted multi-CPU CFS scheduling and displays per-CPU tick timelines and task shares.",
    help:
      "kairos sim FILE\n" +
      "Simulates multi-CPU fair scheduling from a task specification file.",
    examples: [
      {
        label: "2 CPUs, 3 tasks (default)",
        input:
          "cpus 2\n" +
          "task alpha 4 12\n" +
          "task beta 2 12\n" +
          "task gamma 1 12"
      },
      {
        label: "1 CPU, skewed weights",
        input:
          "cpus 1\n" +
          "task heavy 8 16\n" +
          "task medium 2 8\n" +
          "task light 1 4"
      },
      {
        label: "Too many CPUs (error)",
        input:
          "cpus 99\n" +
          "task bad 1 4"
      }
    ]
  });
})();
