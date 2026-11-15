(() => {
  "use strict";

  window.DemoApp.mount({
    id: "pandora",
    name: "Pandora",
    source: "crates/pandora",
    about:
      "Linux namespaces, cgroups v2, pivot_root. A small sealed cavity in rock. Parses and validates container specs and cgroup v2 resource limits (dry-run only).",
    help:
      "rootfs /abs/path\n" +
      "hostname NAME\n" +
      "memory 64M | pids 32 | cpu QUOTA/PERIOD\n" +
      "env KEY=VALUE\n" +
      "cmd ARGS...",
    examples: [
      {
        label: "Alpine box with cgroup limits (default)",
        input:
          "rootfs /var/lib/pandora/alpine\n" +
          "hostname monolith-box\n" +
          "memory 64M\n" +
          "pids 32\n" +
          "cpu 50000/100000\n" +
          "env PATH=/usr/bin:/bin\n" +
          "env TERM=xterm-256color\n" +
          "cmd /bin/sh -c id"
      },
      {
        label: "Minimal unthrottled worker",
        input:
          "rootfs /srv/containers/worker\n" +
          "hostname worker-01\n" +
          "memory 1G\n" +
          "pids max\n" +
          "cmd /usr/bin/worker --port 8080"
      },
      {
        label: "Relative rootfs (error)",
        input:
          "rootfs relative/rootfs\n" +
          "hostname bad\n" +
          "cmd /bin/sh"
      }
    ]
  });
})();
