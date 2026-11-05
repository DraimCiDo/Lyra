"""Trusted repository fixtures only; generated model scripts never run on host."""
import json
from pathlib import Path
import tempfile
import unittest
import zipfile
import lyra_documents as d

class Documents(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name)
    def tearDown(self):self.temp.cleanup()
    def test_docx_edit_preserves_other_runs_and_tables(self):
        from docx import Document
        source=self.root/'исходник.docx';target=self.root/'result.docx'
        doc=Document();p=doc.add_paragraph();p.add_run('Old').bold=True;p.add_run(' preserved').italic=True
        doc.add_table(rows=1,cols=1).cell(0,0).text='Table';doc.save(source)
        d.edit(source,target,[{'paragraph':0,'run':0,'text':'Новый'}])
        result=Document(target)
        self.assertEqual(result.paragraphs[0].runs[0].text,'Новый');self.assertTrue(result.paragraphs[0].runs[0].bold)
        self.assertEqual(result.paragraphs[0].runs[1].text,' preserved');self.assertTrue(result.paragraphs[0].runs[1].italic)
        self.assertEqual(result.tables[0].cell(0,0).text,'Table')
        self.assertEqual(Document(source).paragraphs[0].runs[0].text,'Old')
        self.assertTrue(d.inspect(target)['untrusted'])
    def test_spreadsheet_formulas_and_style_are_preserved(self):
        import openpyxl
        source=self.root/'book.xlsx';target=self.root/'result.xlsx'
        d.create(source,{'sheets':{'Лист':[[1,2,'=SUM(A1:B1)']]}})
        book=openpyxl.load_workbook(source);book['Лист']['B1'].number_format='0.00';book.save(source);book.close()
        d.edit(source,target,[{'sheet':'Лист','cell':'A1','value':7}])
        book=openpyxl.load_workbook(target);self.assertEqual(book['Лист']['C1'].value,'=SUM(A1:B1)');self.assertEqual(book['Лист']['B1'].number_format,'0.00');self.assertEqual(book['Лист']['A1'].value,7);book.close()
    def test_slides_preserve_unmodified_text_and_geometry(self):
        from pptx import Presentation
        source=self.root/'slides.pptx';target=self.root/'result.pptx'
        d.create(source,{'slides':[{'title':'Before','paragraphs':['Keep']} ]})
        old=Presentation(source);pos=(old.slides[0].shapes[0].left,old.slides[0].shapes[0].top)
        d.edit(source,target,[{'slide':0,'shape':0,'paragraph':0,'run':0,'text':'После'}])
        deck=Presentation(target);self.assertEqual(deck.slides[0].shapes.title.text,'После');self.assertEqual(deck.slides[0].placeholders[1].text,'Keep');self.assertEqual((deck.slides[0].shapes[0].left,deck.slides[0].shapes[0].top),pos)
    def test_unicode_pdf_rotation_does_not_claim_visual_acceptance(self):
        source=self.root/'input.pdf';target=self.root/'output.pdf'
        result=d.create(source,{'pages':[['Привет, Lyra!']]})
        self.assertFalse(result['semantic_checked']);self.assertEqual(result['visual_review'],'pending')
        self.assertIn('Привет',d.inspect(source)['pages'][0]['text'])
        d.edit(source,target,[{'operation':'rotate','page':0,'degrees':90}])
        from pypdf import PdfReader
        self.assertEqual(PdfReader(target).pages[0].rotation,90)
    def test_package_traversal_duplicates_and_active_content_are_rejected(self):
        for name in ['../escape','word/vbaProject.bin','word/embeddings/unsafe.bin']:
            source=self.root/'unsafe.docx'
            with zipfile.ZipFile(source,'w') as f:f.writestr('[Content_Types].xml','');f.writestr(name,'payload')
            with self.assertRaises(ValueError):d.inspect(source)
    def test_extraction_requires_bounded_ranges(self):
        path=self.root/'text.txt';path.write_text('a\nb\nc',encoding='utf-8')
        self.assertEqual(d.inspect(path,1,1)['lines'],['b'])
        with self.assertRaises(ValueError):d.inspect(path,0,10000)
if __name__=='__main__':unittest.main()
