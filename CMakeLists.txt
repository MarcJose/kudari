# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT

cmake_minimum_required(VERSION 3.20)
# The version is the one of Cargo.toml, which tools/release.py raises; CMake takes only its
# numbers, without the mark of a pre-release such as -rc.1.
file(STRINGS Cargo.toml KUDARI_VERSION REGEX "^version = " LIMIT_COUNT 1)
string(REGEX REPLACE "^version = \"([0-9]+\\.[0-9]+\\.[0-9]+)[^\"]*\".*" "\\1" KUDARI_VERSION "${KUDARI_VERSION}")
project(kudari VERSION ${KUDARI_VERSION} LANGUAGES CXX)

find_package(ECM 6.0 REQUIRED NO_MODULE)
list(APPEND CMAKE_MODULE_PATH ${ECM_MODULE_PATH})

include(KDEInstallDirs)
include(ECMInstallIcons)
find_package(KF6 6.0 REQUIRED COMPONENTS I18n)
# Optional, as in other KDE programs: without it everything but the man page is built.
find_package(KF6DocTools 6.0)
find_program(CARGO cargo REQUIRED)

set(CARGO_OUTPUT ${CMAKE_BINARY_DIR}/cargo)

add_custom_target(kudari ALL
    COMMAND ${CMAKE_COMMAND} -E env CARGO_TARGET_DIR=${CARGO_OUTPUT} ${CARGO} build --release --locked
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    USES_TERMINAL)

install(PROGRAMS ${CARGO_OUTPUT}/release/kudari DESTINATION ${KDE_INSTALL_BINDIR})
# The full path keeps installations outside the system prefix startable, for example
# from services whose PATH does not contain ~/.local/bin.
configure_file(data/io.github.marcjose.kudari.desktop.in ${CMAKE_BINARY_DIR}/io.github.marcjose.kudari.desktop @ONLY)
install(FILES ${CMAKE_BINARY_DIR}/io.github.marcjose.kudari.desktop DESTINATION ${KDE_INSTALL_APPDIR})
install(FILES data/io.github.marcjose.kudari.metainfo.xml DESTINATION ${KDE_INSTALL_METAINFODIR})
install(FILES data/kudari.notifyrc DESTINATION ${KDE_INSTALL_KNOTIFYRCDIR})
install(FILES data/icons/io.github.marcjose.kudari.svg data/icons/io.github.marcjose.kudari-symbolic.svg
        DESTINATION ${KDE_INSTALL_ICONDIR}/hicolor/scalable/apps)
install(FILES LICENSES/MIT.txt THIRD_PARTY_LICENSES DESTINATION ${KDE_INSTALL_DATAROOTDIR}/doc/kudari)

configure_file(data/io.github.marcjose.kudari.service.in ${CMAKE_BINARY_DIR}/io.github.marcjose.kudari.service @ONLY)
install(FILES ${CMAKE_BINARY_DIR}/io.github.marcjose.kudari.service DESTINATION ${KDE_INSTALL_DBUSSERVICEDIR})

ki18n_install(po)

if(KF6DocTools_FOUND)
    add_subdirectory(docs)
else()
    message(STATUS "KDocTools not found: the man page is not built")
endif()
