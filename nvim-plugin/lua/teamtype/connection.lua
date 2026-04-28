-- SPDX-FileCopyrightText: 2025 blinry <mail@blinry.org>
-- SPDX-FileCopyrightText: 2025 zormit <nt4u@kpvn.de>
--
-- SPDX-License-Identifier: AGPL-3.0-or-later

local M = {}

-- A Connection represents an ative JSON-RPC connection.
local Connection = {}

function Connection:is_connected()
    return self.connection ~= nil
end

function Connection:send_notification(method, params)
    self.connection.notify(method, params)
end

function Connection:send_request(method, params, result_callback, err_callback)
    err_callback = err_callback or function() end
    result_callback = result_callback or function() end

    self.connection.request(method, params, function(err, result)
        if err then
            local error_msg = "[teamtype] Error for '" .. method .. "': " .. err.message
            if err.data and err.data ~= "" then
                error_msg = error_msg .. " (" .. err.data .. ")"
            end
            vim.api.nvim_err_writeln(error_msg)
            err_callback(err)
        end
        if result then
            result_callback(result)
        end
    end)
end

-- Connect to the daemon, and return a handle on the connection.
function M.connect(cmd, directory, on_notification)
    local executable = cmd[1]
    if vim.fn.executable(executable) == 0 then
        vim.api.nvim_err_writeln(
            "Tried to connect to the Teamtype daemon, but `"
                .. executable
                .. "` executable was not found. Make sure that is in your PATH."
        )
        return nil
    end

    local dispatchers = {
        notification = on_notification,
        on_error = function(code, ...)
            print("Teamtype connection error: ", code, vim.inspect({ ... }))
        end,
        on_exit = function(code, _)
            if code == 0 then
                vim.schedule(function()
                    vim.api.nvim_err_writeln(
                        "Connection to Teamtype daemon lost. Probably it crashed or was stopped. Please restart the daemon, then Neovim."
                    )
                    -- TODO: Enable writing here again, so that user can make backup of file?
                end)
            else
                print(
                    "Could not connect to Teamtype daemon. Did you start it (in "
                        .. directory
                        .. ")? To stop trying, remove the .teamtype/ directory."
                )
            end
        end,
    }

    local connection
    local extra_spawn_params = { cwd = directory }

    connection = vim.lsp.rpc.start(cmd, dispatchers, extra_spawn_params)

    print("Connected to Teamtype daemon!")

    local result = { connection = connection }
    setmetatable(result, { __index = Connection })
    return result
end

return M
