// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT
//
// The fills and glyphs of a frame on the graphics card: tools/shaders.sh bakes this into
// glyph.frag.qsb.
#version 440

layout(location = 0) in vec2 sampleCoord;
layout(location = 1) in vec4 color;
layout(location = 2) in float ownColors;

layout(location = 0) out vec4 fragColor;

layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
};

layout(binding = 1) uniform sampler2D atlas;

void main()
{
    // The atlas holds glyphs white, or in their own colors, with their alpha multiplied in;
    // a fill takes a white pixel of it.
    vec4 texel = texture(atlas, sampleCoord);
    fragColor = mix(color * texel.a, texel * qt_Opacity, ownColors);
}
