local mapinfo = {
    name        = "WM-subsystems-test",
    description = "Exercises the new WM-gap subsystems end-to-end: a ScalarValue drives PerlinNoise.frequency via a scalar wire; Equation combines two noises; CoastErosion shapes a shoreline; AutoTexture + LightmapBake produce color/lightmap; ChannelSplit/Merge round-trip the lightmap.",
    mapfile     = "maps/wm-subsystems-test.smf",
    modtype     = 3,
    depend      = {
        "Map Helper v1",
    },

    smf = {
        minheight = -30,
        maxheight = 400,
        smtFileName0 = "maps/wm-subsystems-test.smt",
    },

    teams = {
        [0] = { startPos = { x = 512, z = 512 } },
        [1] = { startPos = { x = 1536, z = 1536 } },
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
