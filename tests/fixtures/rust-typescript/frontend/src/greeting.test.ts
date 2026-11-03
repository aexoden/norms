// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

import { describe, expect, it } from "vitest";

import { greeting } from "./greeting";

describe("greeting", () => {
    it("greets by name", () => {
        expect(greeting("world")).toBe("Hello, world!");
    });
});
