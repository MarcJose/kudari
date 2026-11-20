// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT
//
// The fills and glyphs of a frame on the graphics card: tools/shaders.sh bakes this into
// glyph.vert.qsb.
#version 440

layout(location = 0) in vec4 vertexCoord;
layout(location = 1) in vec2 atlasCoord;
// The color of a fill or glyph, with its alpha multiplied in.
layout(location = 2) in vec4 vertexColor;
// 1 where the colors come from the atlas, as for an emoji, 0 where its alpha shapes them.
layout(location = 3) in vec4 vertexKind;

layout(location = 0) out vec2 sampleCoord;
layout(location = 1) out vec4 color;
layout(location = 2) out float ownColors;

layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
};

out gl_PerVertex { vec4 gl_Position; };

void main()
{
    sampleCoord = atlasCoord;
    color = vertexColor * qt_Opacity;
    ownColors = vertexKind.x;
    gl_Position = qt_Matrix * vertexCoord;
}
