# List available recipes
default:
    @just --list

# Render fixtures to generated/ (all, or the named ones)
render *names:
    scripts/render-fixtures.sh {{names}}
