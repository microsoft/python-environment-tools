from reportlab.pdfgen import canvas
from reportlab.lib.pagesizes import A4
from reportlab.lib.units import cm
from datetime import date
from reportlab.platypus import Table, TableStyle, Paragraph
from reportlab.lib import colors
from reportlab.lib.styles import getSampleStyleSheet

def create_invoice(filename, data):
    c = canvas.Canvas(filename, pagesize=A4)

    # Styles
    styles = getSampleStyleSheet()
    styleN = styles['Normal']
    styleH = styles['Heading1']
    # Header
    c.setFont("Helvetica-Bold", 18)  # Larger font size for header
    c.drawCentredString(10.5*cm, 28*cm, "METANOVA GENERAL TRADING L.L.C - O.P.C")
    c.setFont("Helvetica", 12)
    c.drawCentredString(10.5*cm, 27.2*cm, "Abu Dhabi, UAE")

    # Optional: Add a more stylized logo (if available)
    # Example with a transparent PNG and adjusted positioning for better aesthetics:
    # c.drawImage("logo_transparent.png", 1*cm, 26*cm, width=3*cm, height=3*cm, mask='auto')

    # Invoice Details (using Paragraphs for better styling)
    invoice_info = f"""
    <font size=11>Invoice Date: {date.today()}<br/>
    Invoice No: {data['invoice_number']}<br/>
    Customer Name: {data['customer_name']}<br/></font>
    """
    p = Paragraph(invoice_info, styleN)
    p.wrapOn(c, 5*cm, 20*cm)
    p.drawOn(c, 1.5*cm, 23*cm)

    other_info = f"""
    <font size=11>P.O Box: {data['po_box']}<br/>
    TRN: {data['trn']}<br/></font>
    """
    p = Paragraph(other_info, styleN)
    p.wrapOn(c, 5*cm, 20*cm)
    p.drawOn(c, 11.5*cm, 23*cm)


    # Itemized Table (adjusting table position for better layout)
    # ... (same as before, but you can adjust table.drawOn(c, ...) coordinates)

    # Total (using a Paragraph for consistent styling)
    total_para = f"<font size=12><b>Total Amount: {total_amount:.2f} AED</b></font>"
    p = Paragraph(total_para, styleN)
    p.wrapOn(c, 5*cm, 20*cm)
    p.drawOn(c, 11.5*cm, 13.5*cm)  # Adjust position if needed

    # Footer (optional)
    c.line(1.5*cm, 2*cm, 19.5*cm, 2*cm)  # Thin line at the bottom
    footer_text = "Thank you for your business!"
    c.setFont("Helvetica", 10)  # Smaller font for footer
    c.drawCentredString(10.5*cm, 1.5*cm, footer_text)


    c.save()

# ... (rest of the code is the same)
pip install reportlab
data = {
    'invoice_number': 1001,
    'customer_name': 'ACME Trading',
    'po_box': '12345',
    'trn': '123456789012345',  
    # ... (Add more invoice data including items, quantities, and prices)
}
 data = {
     # ... other data ...
     'items': [
         ["Item 1", 2, 10.50],
         ["Item 2", 5, 15.00],
         # ... more items
     ]
 }
python invoice_generator.py
pip install reportlab
invoice_number: 1001
customer_name: ACME Trading
po_box: 12345
trn: 123456789012345
items: 
    - Item 1,2,10.50
    - Item 2,5,15.00
    - Item 3,1,20.00
