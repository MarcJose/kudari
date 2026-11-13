<?xml version="1.0"?>
<!--
SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
SPDX-License-Identifier: CC0-1.0

Turns the DocBook of the man page into plain HTML for Vale: the prose as paragraphs and
headings, and names of commands, options, functions, files and keys as code, which Vale
leaves alone. tests/check.sh runs it.
-->
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="html" encoding="UTF-8"/>

  <xsl:template match="/">
    <html><body><xsl:apply-templates/></body></html>
  </xsl:template>

  <!-- Data about the page rather than prose. -->
  <xsl:template match="refentryinfo|refmeta|refnamediv/refname|cmdsynopsis"/>

  <xsl:template match="title">
    <h2><xsl:apply-templates/></h2>
  </xsl:template>

  <xsl:template match="para|refpurpose|term|member">
    <p><xsl:apply-templates/></p>
  </xsl:template>

  <xsl:template match="command|option|function|parameter|literal|filename|envar|userinput
                       |arg|keycap|replaceable|systemitem|varname|constant|code">
    <code><xsl:apply-templates/></code>
  </xsl:template>

  <xsl:template match="screen|programlisting">
    <pre><xsl:apply-templates/></pre>
  </xsl:template>
</xsl:stylesheet>
