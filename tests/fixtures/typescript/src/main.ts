// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

import { greeting } from "./greeting";

const app = document.querySelector<HTMLDivElement>("#app");

if (app) {
    app.textContent = greeting("world");
    // The version of this build, for telling which one is deployed.
    app.dataset.version = __APP_VERSION__;
}
