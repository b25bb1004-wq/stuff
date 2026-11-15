(() => {
  "use strict";

  window.DemoApp.mount({
    id: "argo",
    name: "Argo",
    source: "crates/argo",
    about:
      "Signed packages, version solver, registry. A stack of stones that marks the trail. Solves semantic version constraints from an index and manifest to produce a deterministic lockfile.",
    help:
      "argo solve\n" +
      "argo lock\n" +
      "argo install\n" +
      "argo serve",
    examples: [
      {
        label: "Alpha-beta-gamma (default)",
        input:
          "[index]\n" +
          "alpha 1.0.0 : beta ^1.0\n" +
          "alpha 1.1.0 : beta ^1.1\n" +
          "beta 1.0.0\n" +
          "beta 1.1.0 : gamma ^2.0\n" +
          "gamma 2.0.0\n" +
          "gamma 2.1.0\n\n" +
          "[manifest]\n" +
          "[package]\n" +
          'name = "demo-app"\n' +
          'version = "0.1.0"\n\n' +
          "[dependencies]\n" +
          'alpha = "^1.0"\n' +
          'gamma = "2.0.*"'
      },
      {
        label: "Diamond graph",
        input:
          "[index]\n" +
          "web 1.0.0 : http ^1.0; codec ^1.0\n" +
          "http 1.2.0 : core ^1.0\n" +
          "codec 1.1.0 : core ^1.1\n" +
          "core 1.0.0\n" +
          "core 1.2.0\n\n" +
          "[manifest]\n" +
          "[package]\n" +
          'name = "service"\n' +
          'version = "0.2.0"\n\n' +
          "[dependencies]\n" +
          'web = "^1.0"'
      },
      {
        label: "Version conflict (error)",
        input:
          "[index]\n" +
          "alpha 1.0.0 : beta ^1.0\n" +
          "beta 1.0.0\n" +
          "beta 2.0.0\n\n" +
          "[manifest]\n" +
          "[package]\n" +
          'name = "broken"\n' +
          'version = "0.1.0"\n\n' +
          "[dependencies]\n" +
          'alpha = "^1.0"\n' +
          'beta = "^2.0"'
      }
    ]
  });
})();
