local mapinfo = {
    name        = "canis_river",
    description = "bar-editor recreation of World Machine map 'canis_river'. Representative terrain pipeline (the WM source has hundreds of devices; see coverage report). WM author: Peter Sarkozy. WM water level 0.24, sun dir [0.0, 0.0, 1.0].",
    author      = "Peter Sarkozy",
    mapfile     = "maps/canis_river.smf",
    modtype     = 3,
    depend      = {
        "Map Helper v1",
    },

    smf = {
        minheight = -78,
        maxheight = 480,
        smtFileName0 = "maps/canis_river.smt",
    },

    lighting = {
        sunDir = { 0, 0, 1 },
    },

    water = {
        baseColor = { 0.4, 0.55, 0.75 },
    },

    teams = {
        [0] = { startPos = { x = 64, z = 256 } },
        [1] = { startPos = { x = 448, z = 256 } },
    },
}

local function lowerkeys(ta)
    local fix = {}
    for i, v in pairs(ta) do
        if (type(i) == "string") then
            if (i ~= i:lower()) then
                fix[#fix + 1] = i
            end
        end
        if (type(v) == "table") then
            lowerkeys(v)
        end
    end
    for i = 1, #fix do
        local idx = fix[i]
        ta[idx:lower()] = ta[idx]
        ta[idx] = nil
    end
end

lowerkeys(mapinfo)

local function tmerge(t1, t2)
    for i, v in pairs(t2) do
        if (type(v) == "table") then
            t1[i] = t1[i] or {}
            tmerge(t1[i], v)
        else
            t1[i] = v
        end
    end
end

getfenv()["mapinfo"] = mapinfo
local files = VFS.DirList("mapconfig/mapinfo/", "*.lua")
table.sort(files)
for i = 1, #files do
    local newcfg = VFS.Include(files[i])
    if newcfg then
        lowerkeys(newcfg)
        tmerge(mapinfo, newcfg)
    end
end
getfenv()["mapinfo"] = nil

return mapinfo
