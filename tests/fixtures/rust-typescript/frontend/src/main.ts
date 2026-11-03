// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

import { greeting } from "./greeting";

const app = document.querySelector<HTMLDivElement>("#app");

if (app) {
    app.textContent = greeting("world");
}
