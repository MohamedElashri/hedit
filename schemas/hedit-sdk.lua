---@meta
-- Editing SDK 1, manifest API 3. Byte offsets are zero-based UTF-8.
---@class HeditSelection
---@field anchor integer
---@field head integer
---@class HeditDocument
---@field id string Opaque per-invocation document handle; never a filesystem path.
---@field revision string Opaque revision token.
---@field text string Exact authorized document snapshot, at most 64 KiB.
---@field language string
---@field crlf boolean
---@field selections HeditSelection[]
---@class HeditContext
---@field document HeditDocument
---@field settings table<string, boolean|integer|string>
---@field arguments table<string, boolean|integer|string>
---@field storage? table<string, boolean|integer|string> Present only with private command storage authority.
---@class HeditEdit
---@field start integer Inclusive snapshot byte offset.
---@field finish integer Exclusive snapshot byte offset.
---@field text string UTF-8 replacement without NUL.
---@class HeditTransaction
---@field document string
---@field revision string
---@field edits HeditEdit[]
---@field selections? HeditSelection[] Positions in the normalized resulting document.
---@field storage? table<string, boolean|integer|string> Storage-only proposals require empty edits and omitted selections.
hedit = {}
---@param start integer
---@param finish integer
---@param text string
---@return HeditEdit
function hedit.edit(start, finish, text) end
---@param edits HeditEdit[] Ordered, non-overlapping edits; at most 64.
---@param selections? HeditSelection[] At most 16, in the resulting document.
---@return HeditTransaction
function hedit.transaction(edits, selections) end
---@param text string
---@return HeditTransaction
function hedit.replace(text) end
---@param byte integer
---@return integer line Zero-based line.
---@return integer column UTF-8 byte column.
function hedit.line_column(byte) end
---@param line integer
---@param column integer UTF-8 byte column.
---@return integer byte
function hedit.offset(line, column) end
---@param name string Package-local .lua name, never a path or URI.
---@return any
function hedit.import(name) end
---@param name string Resource ID, never a path or URI.
---@return string
function hedit.resource(name) end
