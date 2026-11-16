(() => {
  "use strict";

  window.Kit.mount({
    id: "zeus",
    name: "Zeus",
    source: "crates/zeus",
    about:
      "Type-1 hypervisor on the Silicon H extension. Two guests run side by side in isolated memory, and zeus schedules them and emulates their devices. This page explains it and shows one recorded run.",
    help:
      "zeusctl run [--guest NAME[:MiB]]... [--steps N]\n" +
      "Boots zeus with up to three guests (lithium or titan) on Silicon and prints each guest's console."
  });
})();
