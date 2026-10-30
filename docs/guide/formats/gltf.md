# glTF .gltf and .glb

Export only. One node per visible layer, groups as parent nodes, and linked clones as
instances of one mesh. Colors are vertex colors (default) or a palette texture; layer
materials become PBR materials (metallic, roughness, emission, alpha).

*Scale* defaults to 0.1 m per voxel; *Pivot* as for [OBJ](obj.md). `.glb` is one binary
file; `.gltf` is JSON with a `.bin` file of the same name next to it. Animation, cameras and lights are not written.
