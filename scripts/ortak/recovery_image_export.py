"""Closed raw/gzip image-export metadata and bounded inert validation; never load an image."""
import hashlib
import os
import re
import stat
import time
import zlib

from backup_private_database import Refused

FORMAT = 'ortak-private-image-export/1'
GZIP_HEADER = b'\x1f\x8b\x08\x00\x00\x00\x00\x00\x04\xff'


def require(value, code='image_export_metadata_refused'):
    if not value:raise Refused(code)


def options(gzip_images, output_limit, maximum):
    """Only explicit gzip opts into a smaller physical reservation; raw keeps its existing ceiling."""
    require(type(gzip_images) is bool and type(maximum) is int and maximum>0)
    require(output_limit is None or (gzip_images and type(output_limit) is int and 0<output_limit<=maximum))
    return maximum if output_limit is None else output_limit


def selection(row, maximum):
    """Legacy raw receipts remain exact; new gzip receipts must name their complete versioned shape."""
    base={'path','bytes','sha256','images'}
    require(isinstance(row,dict) and type(row.get('bytes')) is int and 0<row['bytes']<=maximum
        and isinstance(row.get('sha256'),str) and re.fullmatch('[0-9a-f]{64}',row['sha256'])
        and isinstance(row.get('images'),list) and 0<len(row['images'])<=32
        and all(isinstance(image,str) and re.fullmatch('sha256:[0-9a-f]{64}',image) for image in row['images'])
        and len(set(row['images']))==len(row['images']))
    if set(row)==base:
        require(row['path']=='images.tar')
        return {'path':'images.tar','compression':'none','physical_limit':maximum}
    require(set(row)==base|{'format','compression','uncompressed_bytes','uncompressed_sha256','output_limit'}
        and row['format']==FORMAT and row['compression']=='gzip' and row['path']=='images.tar.gz'
        and type(row['output_limit']) is int and row['bytes']<=row['output_limit']<=maximum
        and type(row['uncompressed_bytes']) is int and 0<row['uncompressed_bytes']<=maximum
        and isinstance(row['uncompressed_sha256'],str)
        and re.fullmatch('[0-9a-f]{64}',row['uncompressed_sha256']))
    return {'path':'images.tar.gz','compression':'gzip','physical_limit':row['output_limit']}


def verify_gzip(path, row, maximum, *, seconds=120):
    """Verify one bounded gzip member/footer and exact bytes without extraction or Docker calls."""
    selected=selection(row,maximum)
    if selected['compression']=='none':return selected
    require(type(seconds) in (int,float) and 0<seconds<=900)
    deadline=time.monotonic()+seconds
    before=path.lstat()
    def stamp(value):
        return (value.st_dev,value.st_ino,value.st_size,value.st_mtime_ns,value.st_ctime_ns,value.st_mode,value.st_uid,value.st_nlink)
    require(stat.S_ISREG(before.st_mode) and stat.S_IMODE(before.st_mode)==0o600
        and before.st_nlink==1 and before.st_uid==os.getuid() and before.st_size==row['bytes'],
        'image_export_changed')
    decoder=zlib.decompressobj(31);physical=hashlib.sha256();content=hashlib.sha256()
    count=compressed=0
    try:
        with os.fdopen(os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK),'rb') as stream:
            require(stamp(os.fstat(stream.fileno()))==stamp(before),'image_export_changed')
            while block:=stream.read(65536):
                require(time.monotonic()<deadline,'image_export_deadline')
                if compressed==0:require(block.startswith(GZIP_HEADER),'image_export_gzip_refused')
                compressed+=len(block);require(compressed<=row['bytes'],'image_export_changed');physical.update(block)
                require(not decoder.eof,'image_export_gzip_refused')
                pending=block
                while pending:
                    require(time.monotonic()<deadline,'image_export_deadline')
                    decoded=decoder.decompress(pending,min(65536,row['uncompressed_bytes']-count+1))
                    count+=len(decoded);require(count<=row['uncompressed_bytes'],'image_export_uncompressed_limit')
                    content.update(decoded);pending=decoder.unconsumed_tail
                    require(not decoder.unused_data,'image_export_gzip_refused')
            require(stamp(os.fstat(stream.fileno()))==stamp(before),'image_export_changed')
    except zlib.error:
        raise Refused('image_export_gzip_refused') from None
    require(decoder.eof and count==row['uncompressed_bytes'] and compressed==row['bytes']
        and content.hexdigest()==row['uncompressed_sha256'] and physical.hexdigest()==row['sha256'],
        'image_export_gzip_refused')
    require(stamp(path.lstat())==stamp(before),'image_export_changed')
    return {**selected,'uncompressed_bytes':count,'uncompressed_sha256':content.hexdigest(),
        'footer_verified':True,'image_loading_performed':False}
