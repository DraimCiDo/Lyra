"""Bounded Office/PDF helpers for the isolated Lyra document environment.

All returned text is untrusted document data. No macros, embedded programs,
external resources or model instructions are executed. Rendering is separate
from semantic/visual acceptance. Scripts using this module still run in Docker.
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import zipfile
import warnings

MAX_FILE = 64 * 1024 * 1024
MAX_EXPANDED = 256 * 1024 * 1024
MAX_TEXT = 65536
FORMATS = {'.docx', '.xlsx', '.pptx', '.pdf', '.txt', '.md', '.csv', '.json', '.html'}


def _path(path, output=False):
    path = Path(path)
    if path.is_symlink():
        raise ValueError('Document links are forbidden')
    resolved = path.resolve()
    if os.environ.get('LYRA_DOCUMENT_SANDBOX') == '1':
        roots = [Path('/output')] if output else [Path('/source'), Path('/output')]
        if not any(root == resolved or root in resolved.parents for root in roots):
            raise ValueError('Document path escapes the granted inputs/outputs')
    if output:
        resolved.parent.mkdir(parents=True, exist_ok=True)
    elif not resolved.is_file() or resolved.stat().st_size > MAX_FILE:
        raise ValueError('Expected regular document up to 64 MiB')
    return resolved


def _package(path):
    if path.suffix.lower() not in {'.docx', '.xlsx', '.pptx'}:
        return set()
    with zipfile.ZipFile(path) as package:
        names = set(); total = 0
        for entry in package.infolist():
            name = entry.filename
            if name in names or len(names) >= 10000 or name.startswith('/') or '..' in Path(name).parts or '\\' in name:
                raise ValueError('Unsafe or duplicate Office package entry')
            if (entry.external_attr >> 16) & 0o170000 == 0o120000:
                raise ValueError('Office package contains link')
            names.add(name); total += entry.file_size
            if total > MAX_EXPANDED or entry.file_size > MAX_FILE:
                raise ValueError('Office package expansion exceeds limits')
            if any(part in name.lower() for part in ('vbaproject', 'activex', 'embeddings/', '_xmlsignatures/')):
                raise ValueError('Active/embedded/signed Office content requires a dedicated adapter')
        if '[Content_Types].xml' not in names:
            raise ValueError('Missing Office content types')
        return names


def _bounded(value):
    raw = json.dumps(value, ensure_ascii=False)
    if len(raw.encode()) > MAX_TEXT:
        raise ValueError('Extracted document exceeds 64 KiB; request a smaller range')
    return value


def inspect(path, start=0, count=100):
    path = _path(path); _package(path)
    if not 0 <= start <= 100000 or not 1 <= count <= 200:
        raise ValueError('Expected bounded start/count')
    suffix = path.suffix.lower()
    result = {'path': path.name, 'format': suffix, 'untrusted': True, 'start': start}
    if suffix == '.docx':
        from docx import Document
        doc = Document(path)
        result.update(total=len(doc.paragraphs), paragraphs=[{'paragraph': i, 'style': p.style.name,
                      'runs': [{'run': j, 'text': r.text} for j, r in enumerate(p.runs)]}
                      for i, p in list(enumerate(doc.paragraphs))[start:start+count]],
                      tables=[{'table': i, 'rows': len(t.rows), 'columns': len(t.columns), 'cells':[{'row':r,'column':c,'paragraphs':[{'paragraph':k,'runs':[{'run':n,'text':v.text} for n,v in enumerate(p.runs)]} for k,p in enumerate(t.cell(r,c).paragraphs)]} for r in range(start,min(len(t.rows),start+count)) for c in range(min(len(t.columns),32))]} for i,t in list(enumerate(doc.tables))[:8]])
    elif suffix == '.xlsx':
        import openpyxl
        with warnings.catch_warnings():
            warnings.simplefilter('error')
            book = openpyxl.load_workbook(path, read_only=True, data_only=False, keep_links=False)
        try:
            result['sheets'] = [{'name': s.title, 'rows': s.max_row, 'columns': s.max_column,
                'cells': [{'row': r, 'column': c, 'value': str(s.cell(r,c).value) if s.cell(r,c).value is not None else None}
                    for r in range(start+1, min(s.max_row, start+count)+1) for c in range(1,min(s.max_column,32)+1)]}
                for s in list(book)[:32]]
        finally:
            book.close()
    elif suffix == '.pptx':
        from pptx import Presentation
        deck=Presentation(path)
        result.update(total=len(deck.slides), slides=[{'slide': i, 'shapes': [{'shape': j,
            'paragraphs': [{'paragraph': k, 'runs':[{'run': n,'text':r.text} for n,r in enumerate(p.runs)]}
                for k,p in enumerate(shape.text_frame.paragraphs)]} for j,shape in enumerate(slide.shapes) if shape.has_text_frame]}
            for i,slide in list(enumerate(deck.slides))[start:start+count]])
    elif suffix == '.pdf':
        from pypdf import PdfReader
        doc=PdfReader(path, strict=True)
        if doc.is_encrypted:
            raise ValueError('Encrypted PDF requires explicit decryption workflow')
        result.update(total=len(doc.pages), pages=[{'page':i, 'text':p.extract_text() or '',
            'width':float(p.mediabox.width), 'height':float(p.mediabox.height)}
            for i,p in list(enumerate(doc.pages))[start:start+count]])
    elif suffix in FORMATS:
        lines=path.read_text(encoding='utf-8').splitlines()
        result.update(total=len(lines), lines=lines[start:start+count])
    else:
        raise ValueError('No document adapter for this format')
    return _bounded(result)


def create(path, spec):
    path = _path(path, True); suffix=path.suffix.lower()
    if suffix == '.docx':
        from docx import Document
        doc=Document()
        for paragraph in spec.get('paragraphs',[]):
            if isinstance(paragraph,str): doc.add_paragraph(paragraph)
            else: doc.add_paragraph(paragraph['text'], style=paragraph.get('style'))
        for table in spec.get('tables',[]):
            t=doc.add_table(rows=len(table),cols=max(map(len,table),default=1))
            for i,row in enumerate(table):
                for j,value in enumerate(row):t.cell(i,j).text=str(value)
        doc.save(path)
    elif suffix == '.xlsx':
        import openpyxl
        book=openpyxl.Workbook();book.remove(book.active)
        for name,rows in spec['sheets'].items():
            sheet=book.create_sheet(name)
            for row in rows:sheet.append(row)
        book.save(path)
    elif suffix == '.pptx':
        from pptx import Presentation
        deck=Presentation()
        for item in spec.get('slides',[]):
            slide=deck.slides.add_slide(deck.slide_layouts[1]);slide.shapes.title.text=item.get('title','')
            slide.placeholders[1].text='\n'.join(item.get('paragraphs',[]))
        deck.save(path)
    elif suffix == '.pdf':
        from reportlab.pdfgen import canvas
        from reportlab.pdfbase import pdfmetrics
        from reportlab.pdfbase.ttfonts import TTFont
        font=spec.get('font_path','/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf')
        if not Path(font).is_file():raise ValueError('Unicode PDF font unavailable')
        pdfmetrics.registerFont(TTFont('LyraUnicode',font))
        doc=canvas.Canvas(str(path))
        for page in spec.get('pages',[]):
            doc.setFont('LyraUnicode',12);y=800
            for line in page:
                if y < 35:raise ValueError('PDF text overflows page; split content explicitly')
                doc.drawString(40,y,str(line));y-=18
            doc.showPage()
        doc.save()
    else:
        raise ValueError('Use explicit text serializers for text outputs')
    return validate(path)


def _set_run(paragraph, op):
    index=op['run']
    if not 0 <= index <= len(paragraph.runs):raise ValueError('Run address out of bounds')
    run=paragraph.add_run() if index==len(paragraph.runs) else paragraph.runs[index]
    run.text=op['text']


def edit(source, destination, operations):
    source=_path(source);destination=_path(destination,True)
    if source.suffix.lower()!=destination.suffix.lower():raise ValueError('Editing must preserve format')
    original=_package(source);suffix=source.suffix.lower()
    with warnings.catch_warnings():
        warnings.simplefilter('error')
        if suffix=='.docx':
            from docx import Document
            doc=Document(source)
            for op in operations:
                if 'table' in op:
                    paragraph=doc.tables[op['table']].cell(op['row'],op['column']).paragraphs[op.get('paragraph',0)]
                else:paragraph=doc.paragraphs[op['paragraph']]
                _set_run(paragraph,op)
            doc.save(destination)
        elif suffix=='.xlsx':
            import openpyxl
            book=openpyxl.load_workbook(source, keep_links=True)
            for op in operations:book[op['sheet']][op['cell']]=op['value']
            book.save(destination);book.close()
        elif suffix=='.pptx':
            from pptx import Presentation
            deck=Presentation(source)
            for op in operations:
                _set_run(deck.slides[op['slide']].shapes[op['shape']].text_frame.paragraphs[op['paragraph']],op)
            deck.save(destination)
        elif suffix=='.pdf':
            from pypdf import PdfReader,PdfWriter
            reader=PdfReader(source, strict=True);writer=PdfWriter(clone_from=reader)
            for op in operations:
                kind=op['operation']
                if kind=='rotate':writer.pages[op['page']].rotate(op['degrees'])
                elif kind=='fill_form':writer.update_page_form_field_values(writer.pages[op['page']],op['values'],auto_regenerate=False)
                elif kind=='remove_page':writer.remove_page(op['page'])
                elif kind=='annotation':
                    from pypdf.annotations import Text
                    writer.add_annotation(op['page'],Text(rect=op['rect'],text=op['text']))
                elif kind=='append':writer.append(str(_path(op['path'])))
                else:raise ValueError('Unsupported PDF edit; request a supported operation or explicit recreation')
            writer.write(destination)
        else:raise ValueError('No edit adapter')
    remaining=_package(destination)
    # Missing archive parts may contain unsupported charts, metadata or objects.
    # Preserve the original and reject the transformation instead of flattening it.
    if original-remaining-{'xl/sharedStrings.xml','xl/calcChain.xml'}:
        destination.unlink(missing_ok=True)
        raise ValueError('Transformation removed package components; dedicated adapter required')
    return validate(destination)


def validate(path):
    path=_path(path);_package(path)
    result=inspect(path,0,1)
    return {'path':path.name,'format':result['format'],'structure_checked':True,
            'semantic_checked':False,'visual_review':'pending','bytes':path.stat().st_size}


def render(source, destination):
    source=_path(source);destination=_path(destination,True)
    if source.suffix.lower()=='.pdf':
        if destination.suffix.lower()!='.png':raise ValueError('PDF preview destination must be PNG')
        subprocess.run(['pdftoppm','-f','1','-singlefile','-scale-to','1600','-png',str(source),str(destination.with_suffix(''))],check=True,timeout=60)
    else:
        if destination.suffix.lower()!='.pdf':raise ValueError('Office preview destination must be PDF')
        with tempfile.TemporaryDirectory(prefix='lyra-render-') as folder:
            subprocess.run(['libreoffice','-env:UserInstallation=file://'+folder+'/profile','--headless',
                '--convert-to','pdf','--outdir',folder,str(source)],check=True,timeout=90,stdout=subprocess.DEVNULL)
            generated=Path(folder)/(source.stem+'.pdf')
            if not generated.is_file():raise ValueError('Renderer produced no PDF')
            destination.write_bytes(generated.read_bytes())
    return {'path':destination.name,'rendered':True,'visual_review':'pending'}


def ocr(source, start=0, count=1):
    source=_path(source)
    if source.suffix.lower()!='.pdf' or not 0<=start<10000 or not 1<=count<=10:raise ValueError('OCR requires bounded PDF pages')
    pages=[]
    with tempfile.TemporaryDirectory(prefix='lyra-ocr-') as folder:
        subprocess.run(['pdftoppm','-f',str(start+1),'-l',str(start+count),'-r','150','-png',str(source),folder+'/page'],check=True,timeout=60,stdout=subprocess.DEVNULL)
        for image in sorted(Path(folder).glob('page-*.png')):
            result=subprocess.run(['tesseract',str(image),'stdout','-l','rus+eng'],check=True,capture_output=True,timeout=30)
            pages.append({'image':image.name,'text':result.stdout[:MAX_TEXT].decode('utf-8'),'ocr':True})
    return _bounded({'pages':pages,'untrusted':True,'ocr_accuracy_unverified':True})
