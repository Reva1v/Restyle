# -*- coding: utf-8 -*-
"""Итоговая иконка Restyle: тёмная плашка (#181a1e) + янтарная R (#e8a33d).
На тёмной панели задач плашка сливается с фоном и видна чистая янтарная буква,
на светлой — тёмный значок с янтарной буквой. Каждый размер рисуется отдельно:
даунскейл 256→16 мылит букву.

Пишет:
  src-tauri/icons/tray-{16,20,24,32}.png   — трей (выбор по SM_CXSMICON)
  src-tauri/icons/icon.png, icon.ico, 32x32.png, 64x64.png, 128x128.png,
  128x128@2x.png, Square*Logo.png, StoreLogo.png — значок приложения
Превью: %TEMP%/restyle_icons/final.png
"""
import os, sys
from PIL import Image, ImageDraw, ImageFont

ROOT = r"C:\Users\Reva1v\WebstormProjects\Restyle"
ICONS = os.path.join(ROOT, "src-tauri", "icons")
TMP = os.path.join(os.environ["TEMP"], "restyle_icons")
os.makedirs(TMP, exist_ok=True)

AMBER = (232, 163, 61, 255)   # #e8a33d — акцент тёмной темы приложения
INK = (24, 26, 30, 255)       # #181a1e — поверхность HUD

def plex(weight):
    src = os.path.join(ROOT, "node_modules", "@fontsource", "ibm-plex-sans",
                       "files", f"ibm-plex-sans-latin-{weight}-normal.woff")
    ttf = os.path.join(TMP, f"plex-{weight}.ttf")
    if not os.path.exists(ttf):
        from fontTools.ttLib import TTFont
        f = TTFont(src); f.flavor = None; f.save(ttf)
    return ttf

FONT = plex(700)

def letter_frac(S):
    """Мелкие размеры требуют более крупной буквы, иначе она исчезает."""
    if S <= 16: return .80
    if S <= 20: return .78
    if S <= 24: return .76
    if S <= 48: return .72
    return .68

def render(S, scale=8):
    """Рисуем с запасом и уменьшаем: сглаженные углы и буква без рваных краёв.
    Для 16–32 запас небольшой, чтобы штрих не размывался."""
    big = S * scale
    im = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    d.rounded_rectangle([0, 0, big - 1, big - 1], radius=int(big * .235), fill=INK)
    fnt = ImageFont.truetype(FONT, int(round(big * letter_frac(S))))
    box = d.textbbox((0, 0), "R", font=fnt)
    w, h = box[2] - box[0], box[3] - box[1]
    d.text((round((big - w) / 2 - box[0]), round((big - h) / 2 - box[1])), "R", font=fnt, fill=AMBER)
    return im.resize((S, S), Image.LANCZOS)

TRAY = [16, 20, 24, 32]
APP = {
    "32x32.png": 32, "64x64.png": 64, "128x128.png": 128, "128x128@2x.png": 256,
    "icon.png": 512, "Square30x30Logo.png": 30, "Square44x44Logo.png": 44,
    "Square71x71Logo.png": 71, "Square89x89Logo.png": 89, "Square107x107Logo.png": 107,
    "Square142x142Logo.png": 142, "Square150x150Logo.png": 150,
    "Square284x284Logo.png": 284, "Square310x310Logo.png": 310, "StoreLogo.png": 50,
}

if "--write" in sys.argv:
    for s in TRAY:
        render(s).save(os.path.join(ICONS, f"tray-{s}.png"))
    for name, s in APP.items():
        render(s).save(os.path.join(ICONS, name))
    # ICO: базовой должна быть САМАЯ БОЛЬШАЯ картинка — PIL выбрасывает из
    # `sizes` всё, что крупнее базовой (иначе в файле остаётся один 16x16).
    ico = [render(s) for s in (256, 128, 64, 48, 32, 24, 20, 16)]
    ico[0].save(os.path.join(ICONS, "icon.ico"), format="ICO",
                sizes=[(i.width, i.height) for i in ico], append_images=ico[1:])
    print("icons written to", ICONS)

# превью: трей-размеры на тёмной и светлой полосе + крупный значок
cell, pad = 56, 14
sheet = Image.new("RGBA", (pad * 2 + len(TRAY) * 2 * cell + 300, pad * 2 + max(cell, 256)), (40, 40, 44, 255))
sd = ImageDraw.Draw(sheet)
for col, s in enumerate(TRAY * 2):
    x = pad + col * cell
    bg = (28, 28, 28, 255) if col < len(TRAY) else (243, 243, 243, 255)
    sd.rectangle([x, pad, x + cell - 1, pad + cell - 1], fill=bg)
    sheet.alpha_composite(render(s), (x + (cell - s) // 2, pad + (cell - s) // 2))
sheet.alpha_composite(render(256), (pad * 2 + len(TRAY) * 2 * cell, pad))
sheet.save(os.path.join(TMP, "final.png"))
print(os.path.join(TMP, "final.png"))
