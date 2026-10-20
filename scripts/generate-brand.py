from pathlib import Path
root=Path(__file__).resolve().parents[1] / 'assets'
mark='<path d="M14 14h8v13h13l6 6v17h-8V35H22v15h-8z" fill="{primary}"/><path d="M31 14h19v4H31zm7 7h12v4H38z" fill="{secondary}"/>'
# Custom outline lettering. No installed fonts or external assets required.
word='''<path d="M0 4h8v24c4-5 9-7 15-7 12 0 19 8 19 21v26h-8V42c0-9-4-14-12-14S8 34 8 42v26H0z"/>
<path fill-rule="evenodd" d="M77 21c14 0 23 10 23 25v4H62c1 8 6 13 15 13 6 0 11-2 16-6l5 6c-6 5-13 8-21 8-15 0-24-10-24-25s10-25 24-25zm0 7c-8 0-13 5-15 15h30c-1-10-7-15-15-15z"/>
<path fill-rule="evenodd" d="M147 4h8v64h-8v-7c-4 6-10 10-18 10-13 0-23-10-23-25s10-25 23-25c8 0 14 4 18 9zm-17 24c-9 0-16 7-16 18s7 18 16 18 17-7 17-18-8-18-17-18z"/>
<path d="M169 23h8v45h-8zm0-19h8v9h-8zM195 10h8v13h14v7h-14v24c0 7 2 10 8 10 2 0 4 0 6-1v7c-3 1-5 1-8 1-10 0-14-5-14-16V30h-8v-7h8z"/>'''
def svg(name,w,h,body,title):
    (root/name).write_text(f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" role="img" aria-labelledby="title"><title id="title">{title}</title>{body}</svg>\n')
ink='#10161F';mint='#64D8CB';frost='#9AEBDF';white='#DCE6F0'
svg('hedit.svg',128,128,'<g transform="scale(2)"><rect width="64" height="64" rx="14" fill="'+ink+'"/>'+mark.format(primary=mint,secondary=frost)+'</g>','hedit app icon')
svg('hedit-mark.svg',64,64,mark.format(primary=mint,secondary=frost),'hedit buffer mark')
svg('hedit-mark-mono.svg',64,64,mark.format(primary='currentColor',secondary='currentColor'),'hedit monochrome mark')
for suffix,fg,accent,secondary in [('dark',white,mint,frost),('light','#243832','#147568','#59776B')]:
    svg(f'hedit-wordmark-{suffix}.svg',280,76,f'<g transform="translate(4 0)" fill="{fg}">{word}</g>','hedit wordmark')
    svg(f'hedit-lockup-{suffix}.svg',376,96,f'<g transform="translate(0 8) scale(1.25)">'+mark.format(primary=accent,secondary=secondary)+f'</g><g transform="translate(112 9)" fill="{fg}">{word}</g>','hedit logo and wordmark')
svg('hedit-logo-mono.svg',376,96,'<g transform="translate(0 8) scale(1.25)">'+mark.format(primary='currentColor',secondary='currentColor')+f'</g><g transform="translate(112 9)" fill="currentColor">{word}</g>','hedit monochrome logo')
# One source-of-truth showcase for README/release materials.
body=f'<rect width="1120" height="680" rx="28" fill="{ink}"/>'
body+='<g transform="translate(58 58)"><rect width="76" height="76" rx="18" fill="#16202B"/>'+f'<g transform="translate(0 0) scale(1.1875)">{mark.format(primary=mint,secondary=frost)}</g></g>'
body+=f'<g transform="translate(58 177) scale(1.8)" fill="{white}">{word}</g>'
body+='<text x="62" y="357" font-family="DejaVu Sans,sans-serif" font-size="26" fill="#8DA0B4">Keep your train of thought.</text>'
body+='<text x="62" y="408" font-family="DejaVu Sans,sans-serif" font-size="15" fill="#8DA0B4">A native home for logs, long-lived buffers, and CUDA.</text>'
body+='<path d="M62 455h996" stroke="#2B3949"/>'
for i,(name,color) in enumerate([('INK',ink),('SURFACE','#161E29'),('SEA GLASS',mint),('FROST',frost),('SLATE','#8DA0B4')]):
    x=62+i*202
    body+=f'<rect x="{x}" y="490" width="178" height="76" rx="10" fill="{color}" stroke="#2B3949"/><text x="{x}" y="594" fill="{white}" font-family="DejaVu Sans,sans-serif" font-size="12">{name}</text><text x="{x}" y="616" fill="#8DA0B4" font-family="DejaVu Sans Mono,monospace" font-size="12">{color.upper()}</text>'
body+='<g transform="translate(775 75) scale(3.6)">'+mark.format(primary=mint,secondary=frost)+'</g>'
svg('brand-board.svg',1120,680,body,'hedit brand identity: buffer mark, custom lettering, and palette')
