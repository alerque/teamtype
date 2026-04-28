-- SPDX-FileCopyrightText: 2025 blinry <mail@blinry.org>
-- SPDX-FileCopyrightText: 2025 zormit <nt4u@kpvn.de>
-- SPDX-FileCopyrightText: 2026 Caleb Maclennan <caleb@alerque.com>
--
-- SPDX-License-Identifier: AGPL-3.0-or-later

local M = {}

-- Recusively scan up directories. If we find a .teamtype directory on any level, return its parent, and nil otherwise.
function M.find_directory(filename, marker)
    return vim.fs.root(filename, marker)
end

return M
