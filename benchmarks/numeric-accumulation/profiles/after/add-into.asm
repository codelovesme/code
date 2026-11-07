
/var/tmp/code-263-profile-after/threshold-full:     file format elf64-x86-64


Disassembly of section .init:

Disassembly of section .plt:

Disassembly of section .plt.got:

Disassembly of section .text:

000000000000ddd0 <code_add_into>:
    ddd0:	41 57                	push   %r15
    ddd2:	49 89 ff             	mov    %rdi,%r15
    ddd5:	41 56                	push   %r14
    ddd7:	49 89 f6             	mov    %rsi,%r14
    ddda:	41 55                	push   %r13
    dddc:	41 54                	push   %r12
    ddde:	55                   	push   %rbp
    dddf:	53                   	push   %rbx
    dde0:	48 81 ec b8 00 00 00 	sub    $0xb8,%rsp
    dde7:	8b 07                	mov    (%rdi),%eax
    dde9:	85 c0                	test   %eax,%eax
    ddeb:	75 33                	jne    de20 <code_add_into+0x50>
    dded:	44 8b 06             	mov    (%rsi),%r8d
    ddf0:	45 85 c0             	test   %r8d,%r8d
    ddf3:	0f 84 b7 00 00 00    	je     deb0 <code_add_into+0xe0>
    ddf9:	48 81 c4 b8 00 00 00 	add    $0xb8,%rsp
    de00:	4c 89 f2             	mov    %r14,%rdx
    de03:	4c 89 fe             	mov    %r15,%rsi
    de06:	4c 89 ff             	mov    %r15,%rdi
    de09:	5b                   	pop    %rbx
    de0a:	5d                   	pop    %rbp
    de0b:	41 5c                	pop    %r12
    de0d:	41 5d                	pop    %r13
    de0f:	41 5e                	pop    %r14
    de11:	41 5f                	pop    %r15
    de13:	e9 48 f3 ff ff       	jmp    d160 <code_add>
    de18:	0f 1f 84 00 00 00 00 	nopl   0x0(%rax,%rax,1)
    de1f:	00 
    de20:	48 39 f7             	cmp    %rsi,%rdi
    de23:	74 d4                	je     ddf9 <code_add_into+0x29>
    de25:	83 f8 04             	cmp    $0x4,%eax
    de28:	0f 85 a2 01 00 00    	jne    dfd0 <code_add_into+0x200>
    de2e:	8b 47 04             	mov    0x4(%rdi),%eax
    de31:	85 c0                	test   %eax,%eax
    de33:	0f 84 9f 00 00 00    	je     ded8 <code_add_into+0x108>
    de39:	4c 8b 6f 08          	mov    0x8(%rdi),%r13
    de3d:	49 8b 6d f0          	mov    -0x10(%r13),%rbp
    de41:	48 83 fd 01          	cmp    $0x1,%rbp
    de45:	75 b2                	jne    ddf9 <code_add_into+0x29>
    de47:	83 3e 04             	cmpl   $0x4,(%rsi)
    de4a:	0f 84 40 03 00 00    	je     e190 <code_add_into+0x3c0>
    de50:	49 8b 5d 00          	mov    0x0(%r13),%rbx
    de54:	4c 8d 63 01          	lea    0x1(%rbx),%r12
    de58:	4c 89 e0             	mov    %r12,%rax
    de5b:	49 8d 7d f0          	lea    -0x10(%r13),%rdi
    de5f:	48 c1 e0 04          	shl    $0x4,%rax
    de63:	48 89 7c 24 08       	mov    %rdi,0x8(%rsp)
    de68:	48 8d 50 20          	lea    0x20(%rax),%rdx
    de6c:	48 89 14 24          	mov    %rdx,(%rsp)
    de70:	e8 ab 42 ff ff       	call   2120 <malloc_usable_size@plt>
    de75:	48 8b 14 24          	mov    (%rsp),%rdx
    de79:	48 39 d0             	cmp    %rdx,%rax
    de7c:	0f 83 cc 00 00 00    	jae    df4e <code_add_into+0x17e>
    de82:	48 01 c0             	add    %rax,%rax
    de85:	48 8b 7c 24 08       	mov    0x8(%rsp),%rdi
    de8a:	48 39 d0             	cmp    %rdx,%rax
    de8d:	48 0f 43 d0          	cmovae %rax,%rdx
    de91:	48 89 d6             	mov    %rdx,%rsi
    de94:	e8 57 43 ff ff       	call   21f0 <realloc@plt>
    de99:	48 85 c0             	test   %rax,%rax
    de9c:	0f 84 17 08 00 00    	je     e6b9 <code_add_into+0x8e9>
    dea2:	4c 8d 68 10          	lea    0x10(%rax),%r13
    dea6:	e9 a3 00 00 00       	jmp    df4e <code_add_into+0x17e>
    deab:	0f 1f 44 00 00       	nopl   0x0(%rax,%rax,1)
    deb0:	f2 0f 10 47 08       	movsd  0x8(%rdi),%xmm0
    deb5:	f2 0f 58 46 08       	addsd  0x8(%rsi),%xmm0
    deba:	f2 0f 11 47 08       	movsd  %xmm0,0x8(%rdi)
    debf:	48 81 c4 b8 00 00 00 	add    $0xb8,%rsp
    dec6:	5b                   	pop    %rbx
    dec7:	5d                   	pop    %rbp
    dec8:	41 5c                	pop    %r12
    deca:	41 5d                	pop    %r13
    decc:	41 5e                	pop    %r14
    dece:	41 5f                	pop    %r15
    ded0:	c3                   	ret
    ded1:	0f 1f 80 00 00 00 00 	nopl   0x0(%rax)
    ded8:	83 3e 04             	cmpl   $0x4,(%rsi)
    dedb:	0f 84 af 02 00 00    	je     e190 <code_add_into+0x3c0>
    dee1:	48 8b 47 08          	mov    0x8(%rdi),%rax
    dee5:	48 85 c0             	test   %rax,%rax
    dee8:	0f 84 02 03 00 00    	je     e1f0 <code_add_into+0x420>
    deee:	48 8b 18             	mov    (%rax),%rbx
    def1:	bd 01 00 00 00       	mov    $0x1,%ebp
    def6:	4c 8d 63 01          	lea    0x1(%rbx),%r12
    defa:	b8 04 00 00 00       	mov    $0x4,%eax
    deff:	49 39 c4             	cmp    %rax,%r12
    df02:	49 0f 4d c4          	cmovge %r12,%rax
    df06:	48 89 c7             	mov    %rax,%rdi
    df09:	48 89 c2             	mov    %rax,%rdx
    df0c:	48 c1 e7 04          	shl    $0x4,%rdi
    df10:	48 83 c7 20          	add    $0x20,%rdi
    df14:	48 89 14 24          	mov    %rdx,(%rsp)
    df18:	e8 c3 42 ff ff       	call   21e0 <malloc@plt>
    df1d:	48 85 c0             	test   %rax,%rax
    df20:	0f 84 93 07 00 00    	je     e6b9 <code_add_into+0x8e9>
    df26:	66 0f 6f 05 12 31 00 	movdqa 0x3112(%rip),%xmm0        # 11040 <no_value+0x7c0>
    df2d:	00 
    df2e:	0f 11 00             	movups %xmm0,(%rax)
    df31:	f0 48 83 05 ee f1 00 	lock addq $0x1,0xf1ee(%rip)        # 1d128 <live_blocks>
    df38:	00 01 
    df3a:	48 8b 14 24          	mov    (%rsp),%rdx
    df3e:	48 c7 40 18 00 00 00 	movq   $0x0,0x18(%rax)
    df45:	00 
    df46:	4c 8d 68 10          	lea    0x10(%rax),%r13
    df4a:	48 89 50 10          	mov    %rdx,0x10(%rax)
    df4e:	48 85 ed             	test   %rbp,%rbp
    df51:	7e 62                	jle    dfb5 <code_add_into+0x1e5>
    df53:	48 c1 e3 04          	shl    $0x4,%rbx
    df57:	31 d2                	xor    %edx,%edx
    df59:	31 f6                	xor    %esi,%esi
    df5b:	4d 8d 4c 1d 00       	lea    0x0(%r13,%rbx,1),%r9
    df60:	41 83 3e 04          	cmpl   $0x4,(%r14)
    df64:	4c 89 f0             	mov    %r14,%rax
    df67:	75 12                	jne    df7b <code_add_into+0x1ab>
    df69:	49 8b 46 08          	mov    0x8(%r14),%rax
    df6d:	48 85 c0             	test   %rax,%rax
    df70:	48 8d 78 10          	lea    0x10(%rax),%rdi
    df74:	48 0f 45 c7          	cmovne %rdi,%rax
    df78:	48 01 d0             	add    %rdx,%rax
    df7b:	8b 78 04             	mov    0x4(%rax),%edi
    df7e:	85 ff                	test   %edi,%edi
    df80:	74 1c                	je     df9e <code_add_into+0x1ce>
    df82:	8b 38                	mov    (%rax),%edi
    df84:	83 ff 01             	cmp    $0x1,%edi
    df87:	74 0c                	je     df95 <code_add_into+0x1c5>
    df89:	83 ef 04             	sub    $0x4,%edi
    df8c:	83 ff 01             	cmp    $0x1,%edi
    df8f:	0f 87 95 45 ff ff    	ja     252a <code_add_into.cold>
    df95:	48 8b 78 08          	mov    0x8(%rax),%rdi
    df99:	48 83 47 f0 01       	addq   $0x1,-0x10(%rdi)
    df9e:	f3 0f 6f 00          	movdqu (%rax),%xmm0
    dfa2:	48 83 c6 01          	add    $0x1,%rsi
    dfa6:	41 0f 11 44 11 10    	movups %xmm0,0x10(%r9,%rdx,1)
    dfac:	48 83 c2 10          	add    $0x10,%rdx
    dfb0:	48 39 f5             	cmp    %rsi,%rbp
    dfb3:	75 ab                	jne    df60 <code_add_into+0x190>
    dfb5:	4d 89 65 00          	mov    %r12,0x0(%r13)
    dfb9:	4d 89 6f 08          	mov    %r13,0x8(%r15)
    dfbd:	41 c7 47 04 01 00 00 	movl   $0x1,0x4(%r15)
    dfc4:	00 
    dfc5:	e9 f5 fe ff ff       	jmp    debf <code_add_into+0xef>
    dfca:	66 0f 1f 44 00 00    	nopw   0x0(%rax,%rax,1)
    dfd0:	83 f8 01             	cmp    $0x1,%eax
    dfd3:	0f 84 17 01 00 00    	je     e0f0 <code_add_into+0x320>
    dfd9:	83 f8 05             	cmp    $0x5,%eax
    dfdc:	0f 85 17 fe ff ff    	jne    ddf9 <code_add_into+0x29>
    dfe2:	83 3e 05             	cmpl   $0x5,(%rsi)
    dfe5:	0f 85 0e fe ff ff    	jne    ddf9 <code_add_into+0x29>
    dfeb:	8b 4f 04             	mov    0x4(%rdi),%ecx
    dfee:	85 c9                	test   %ecx,%ecx
    dff0:	0f 84 03 fe ff ff    	je     ddf9 <code_add_into+0x29>
    dff6:	48 8b 47 08          	mov    0x8(%rdi),%rax
    dffa:	48 83 78 f0 01       	cmpq   $0x1,-0x10(%rax)
    dfff:	0f 85 f4 fd ff ff    	jne    ddf9 <code_add_into+0x29>
    e005:	48 8b 5e 08          	mov    0x8(%rsi),%rbx
    e009:	48 85 db             	test   %rbx,%rbx
    e00c:	0f 84 fa 01 00 00    	je     e20c <code_add_into+0x43c>
    e012:	48 8b 03             	mov    (%rbx),%rax
    e015:	48 83 f8 10          	cmp    $0x10,%rax
    e019:	0f 8e ed 01 00 00    	jle    e20c <code_add_into+0x43c>
    e01f:	48 8d 3c c5 00 00 00 	lea    0x0(,%rax,8),%rdi
    e026:	00 
    e027:	e8 b4 41 ff ff       	call   21e0 <malloc@plt>
    e02c:	48 89 04 24          	mov    %rax,(%rsp)
    e030:	48 85 c0             	test   %rax,%rax
    e033:	0f 84 80 06 00 00    	je     e6b9 <code_add_into+0x8e9>
    e039:	48 c7 44 24 08 00 00 	movq   $0x0,0x8(%rsp)
    e040:	00 00 
    e042:	45 31 e4             	xor    %r12d,%r12d
    e045:	48 c7 44 24 10 00 00 	movq   $0x0,0x10(%rsp)
    e04c:	00 00 
    e04e:	48 85 db             	test   %rbx,%rbx
    e051:	0f 84 da 05 00 00    	je     e631 <code_add_into+0x861>
    e057:	4c 3b 23             	cmp    (%rbx),%r12
    e05a:	0f 8d d1 05 00 00    	jge    e631 <code_add_into+0x861>
    e060:	48 8b 53 08          	mov    0x8(%rbx),%rdx
    e064:	4a 8d 04 e5 00 00 00 	lea    0x0(,%r12,8),%rax
    e06b:	00 
    e06c:	48 89 44 24 18       	mov    %rax,0x18(%rsp)
    e071:	4a 8b 1c e2          	mov    (%rdx,%r12,8),%rbx
    e075:	49 89 d5             	mov    %rdx,%r13
    e078:	48 8d 2c 02          	lea    (%rdx,%rax,1),%rbp
    e07c:	4d 85 e4             	test   %r12,%r12
    e07f:	74 2c                	je     e0ad <code_add_into+0x2dd>
    e081:	66 66 2e 0f 1f 84 00 	data16 cs nopw 0x0(%rax,%rax,1)
    e088:	00 00 00 00 
    e08c:	0f 1f 40 00          	nopl   0x0(%rax)
    e090:	49 8b 7d 00          	mov    0x0(%r13),%rdi
    e094:	48 89 de             	mov    %rbx,%rsi
    e097:	e8 c4 40 ff ff       	call   2160 <strcmp@plt>
    e09c:	85 c0                	test   %eax,%eax
    e09e:	0f 84 2c 01 00 00    	je     e1d0 <code_add_into+0x400>
    e0a4:	49 83 c5 08          	add    $0x8,%r13
    e0a8:	4c 39 ed             	cmp    %r13,%rbp
    e0ab:	75 e3                	jne    e090 <code_add_into+0x2c0>
    e0ad:	8b 53 fc             	mov    -0x4(%rbx),%edx
    e0b0:	48 89 de             	mov    %rbx,%rsi
    e0b3:	4c 89 ff             	mov    %r15,%rdi
    e0b6:	e8 95 5d ff ff       	call   3e50 <field_position>
    e0bb:	48 8b 0c 24          	mov    (%rsp),%rcx
    e0bf:	4a 89 04 e1          	mov    %rax,(%rcx,%r12,8)
    e0c3:	41 8b 0e             	mov    (%r14),%ecx
    e0c6:	8d 51 fc             	lea    -0x4(%rcx),%edx
    e0c9:	48 85 c0             	test   %rax,%rax
    e0cc:	0f 88 48 01 00 00    	js     e21a <code_add_into+0x44a>
    e0d2:	83 fa 01             	cmp    $0x1,%edx
    e0d5:	0f 87 56 05 00 00    	ja     e631 <code_add_into+0x861>
    e0db:	49 8b 5e 08          	mov    0x8(%r14),%rbx
    e0df:	49 83 c4 01          	add    $0x1,%r12
    e0e3:	e9 66 ff ff ff       	jmp    e04e <code_add_into+0x27e>
    e0e8:	0f 1f 84 00 00 00 00 	nopl   0x0(%rax,%rax,1)
    e0ef:	00 
    e0f0:	83 3e 01             	cmpl   $0x1,(%rsi)
    e0f3:	0f 85 00 fd ff ff    	jne    ddf9 <code_add_into+0x29>
    e0f9:	8b 77 04             	mov    0x4(%rdi),%esi
    e0fc:	85 f6                	test   %esi,%esi
    e0fe:	74 0f                	je     e10f <code_add_into+0x33f>
    e100:	48 8b 47 08          	mov    0x8(%rdi),%rax
    e104:	48 83 78 f0 01       	cmpq   $0x1,-0x10(%rax)
    e109:	0f 85 ea fc ff ff    	jne    ddf9 <code_add_into+0x29>
    e10f:	48 8d 5c 24 30       	lea    0x30(%rsp),%rbx
    e114:	4c 89 fe             	mov    %r15,%rsi
    e117:	48 89 df             	mov    %rbx,%rdi
    e11a:	e8 31 64 ff ff       	call   4550 <append_buffer>
    e11f:	4c 89 f7             	mov    %r14,%rdi
    e122:	e8 f9 4f ff ff       	call   3120 <text_length>
    e127:	49 8b 76 08          	mov    0x8(%r14),%rsi
    e12b:	48 89 df             	mov    %rbx,%rdi
    e12e:	48 89 c2             	mov    %rax,%rdx
    e131:	e8 da 6e ff ff       	call   5010 <text_push>
    e136:	31 d2                	xor    %edx,%edx
    e138:	48 8d 35 15 22 00 00 	lea    0x2215(%rip),%rsi        # 10354 <_fini+0x4f4>
    e13f:	48 89 df             	mov    %rbx,%rdi
    e142:	e8 c9 6e ff ff       	call   5010 <text_push>
    e147:	48 8b 4c 24 30       	mov    0x30(%rsp),%rcx
    e14c:	48 8b 54 24 38       	mov    0x38(%rsp),%rdx
    e151:	48 8b 44 24 40       	mov    0x40(%rsp),%rax
    e156:	c6 04 11 00          	movb   $0x0,(%rcx,%rdx,1)
    e15a:	48 85 c0             	test   %rax,%rax
    e15d:	74 0b                	je     e16a <code_add_into+0x39a>
    e15f:	48 0f bd c0          	bsr    %rax,%rax
    e163:	83 c0 01             	add    $0x1,%eax
    e166:	48 c1 e0 3a          	shl    $0x3a,%rax
    e16a:	48 83 c2 01          	add    $0x1,%rdx
    e16e:	48 09 c2             	or     %rax,%rdx
    e171:	48 b8 01 00 00 00 01 	movabs $0x100000001,%rax
    e178:	00 00 00 
    e17b:	48 89 51 f8          	mov    %rdx,-0x8(%rcx)
    e17f:	49 89 07             	mov    %rax,(%r15)
    e182:	49 89 4f 08          	mov    %rcx,0x8(%r15)
    e186:	e9 34 fd ff ff       	jmp    debf <code_add_into+0xef>
    e18b:	0f 1f 44 00 00       	nopl   0x0(%rax,%rax,1)
    e190:	49 8b 56 08          	mov    0x8(%r14),%rdx
    e194:	48 85 d2             	test   %rdx,%rdx
    e197:	0f 84 22 fd ff ff    	je     debf <code_add_into+0xef>
    e19d:	48 8b 2a             	mov    (%rdx),%rbp
    e1a0:	48 85 ed             	test   %rbp,%rbp
    e1a3:	0f 84 16 fd ff ff    	je     debf <code_add_into+0xef>
    e1a9:	49 8b 57 08          	mov    0x8(%r15),%rdx
    e1ad:	48 85 d2             	test   %rdx,%rdx
    e1b0:	0f 84 fc 04 00 00    	je     e6b2 <code_add_into+0x8e2>
    e1b6:	48 8b 1a             	mov    (%rdx),%rbx
    e1b9:	4c 8d 64 1d 00       	lea    0x0(%rbp,%rbx,1),%r12
    e1be:	85 c0                	test   %eax,%eax
    e1c0:	0f 84 34 fd ff ff    	je     defa <code_add_into+0x12a>
    e1c6:	4d 8b 6f 08          	mov    0x8(%r15),%r13
    e1ca:	e9 89 fc ff ff       	jmp    de58 <code_add_into+0x88>
    e1cf:	90                   	nop
    e1d0:	48 8b 3c 24          	mov    (%rsp),%rdi
    e1d4:	48 8d 44 24 30       	lea    0x30(%rsp),%rax
    e1d9:	48 39 c7             	cmp    %rax,%rdi
    e1dc:	0f 84 17 fc ff ff    	je     ddf9 <code_add_into+0x29>
    e1e2:	e8 69 3e ff ff       	call   2050 <free@plt>
    e1e7:	e9 0d fc ff ff       	jmp    ddf9 <code_add_into+0x29>
    e1ec:	0f 1f 40 00          	nopl   0x0(%rax)
    e1f0:	bf 60 00 00 00       	mov    $0x60,%edi
    e1f5:	ba 04 00 00 00       	mov    $0x4,%edx
    e1fa:	bd 01 00 00 00       	mov    $0x1,%ebp
    e1ff:	31 db                	xor    %ebx,%ebx
    e201:	41 bc 01 00 00 00    	mov    $0x1,%r12d
    e207:	e9 08 fd ff ff       	jmp    df14 <code_add_into+0x144>
    e20c:	48 8d 44 24 30       	lea    0x30(%rsp),%rax
    e211:	48 89 04 24          	mov    %rax,(%rsp)
    e215:	e9 1f fe ff ff       	jmp    e039 <code_add_into+0x269>
    e21a:	48 83 44 24 10 01    	addq   $0x1,0x10(%rsp)
    e220:	83 fa 01             	cmp    $0x1,%edx
    e223:	77 32                	ja     e257 <code_add_into+0x487>
    e225:	49 8b 5e 08          	mov    0x8(%r14),%rbx
    e229:	48 89 d8             	mov    %rbx,%rax
    e22c:	48 85 db             	test   %rbx,%rbx
    e22f:	74 04                	je     e235 <code_add_into+0x465>
    e231:	48 8b 43 08          	mov    0x8(%rbx),%rax
    e235:	48 8b 4c 24 18       	mov    0x18(%rsp),%rcx
    e23a:	48 8b 3c 08          	mov    (%rax,%rcx,1),%rdi
    e23e:	e8 9d 3e ff ff       	call   20e0 <strlen@plt>
    e243:	48 8b 4c 24 08       	mov    0x8(%rsp),%rcx
    e248:	48 8d 44 01 05       	lea    0x5(%rcx,%rax,1),%rax
    e24d:	48 89 44 24 08       	mov    %rax,0x8(%rsp)
    e252:	e9 88 fe ff ff       	jmp    e0df <code_add_into+0x30f>
    e257:	48 8b 44 24 18       	mov    0x18(%rsp),%rax
    e25c:	48 8b 38             	mov    (%rax),%rdi
    e25f:	e8 7c 3e ff ff       	call   20e0 <strlen@plt>
    e264:	48 8b 4c 24 08       	mov    0x8(%rsp),%rcx
    e269:	48 8d 44 01 05       	lea    0x5(%rcx,%rax,1),%rax
    e26e:	48 89 44 24 08       	mov    %rax,0x8(%rsp)
    e273:	4c 89 ff             	mov    %r15,%rdi
    e276:	e8 d5 4a ff ff       	call   2d50 <drop_object_index>
    e27b:	4d 8b 67 08          	mov    0x8(%r15),%r12
    e27f:	bf 10 00 00 00       	mov    $0x10,%edi
    e284:	49 8b 74 24 08       	mov    0x8(%r12),%rsi
    e289:	49 8d 54 24 10       	lea    0x10(%r12),%rdx
    e28e:	48 89 f0             	mov    %rsi,%rax
    e291:	48 29 d0             	sub    %rdx,%rax
    e294:	48 99                	cqto
    e296:	48 f7 ff             	idiv   %rdi
    e299:	48 89 c5             	mov    %rax,%rbp
    e29c:	49 8b 04 24          	mov    (%r12),%rax
    e2a0:	48 8b 5c c6 f8       	mov    -0x8(%rsi,%rax,8),%rbx
    e2a5:	48 89 df             	mov    %rbx,%rdi
    e2a8:	e8 33 3e ff ff       	call   20e0 <strlen@plt>
    e2ad:	4c 8d 6c 03 01       	lea    0x1(%rbx,%rax,1),%r13
    e2b2:	49 8d 44 24 f0       	lea    -0x10(%r12),%rax
    e2b7:	48 89 c7             	mov    %rax,%rdi
    e2ba:	48 89 44 24 28       	mov    %rax,0x28(%rsp)
    e2bf:	e8 5c 3e ff ff       	call   2120 <malloc_usable_size@plt>
    e2c4:	48 89 c2             	mov    %rax,%rdx
    e2c7:	48 8b 44 24 10       	mov    0x10(%rsp),%rax
    e2cc:	49 03 04 24          	add    (%r12),%rax
    e2d0:	48 39 c5             	cmp    %rax,%rbp
    e2d3:	7c 19                	jl     e2ee <code_add_into+0x51e>
    e2d5:	4c 89 e6             	mov    %r12,%rsi
    e2d8:	48 8b 4c 24 08       	mov    0x8(%rsp),%rcx
    e2dd:	4c 29 ee             	sub    %r13,%rsi
    e2e0:	48 8d 54 16 f0       	lea    -0x10(%rsi,%rdx,1),%rdx
    e2e5:	48 39 ca             	cmp    %rcx,%rdx
    e2e8:	0f 8d 0d 01 00 00    	jge    e3fb <code_add_into+0x62b>
    e2ee:	48 83 f8 03          	cmp    $0x3,%rax
    e2f2:	48 8d 1c 00          	lea    (%rax,%rax,1),%rbx
    e2f6:	b8 04 00 00 00       	mov    $0x4,%eax
    e2fb:	4c 89 ea             	mov    %r13,%rdx
    e2fe:	48 0f 4e d8          	cmovle %rax,%rbx
    e302:	49 8b 44 24 08       	mov    0x8(%r12),%rax
    e307:	48 8d 04 e8          	lea    (%rax,%rbp,8),%rax
    e30b:	48 29 c2             	sub    %rax,%rdx
    e30e:	48 8b 44 24 08       	mov    0x8(%rsp),%rax
    e313:	48 01 c2             	add    %rax,%rdx
    e316:	48 6b c3 18          	imul   $0x18,%rbx,%rax
    e31a:	48 8d 7c 50 30       	lea    0x30(%rax,%rdx,2),%rdi
    e31f:	e8 bc 3e ff ff       	call   21e0 <malloc@plt>
    e324:	48 89 c1             	mov    %rax,%rcx
    e327:	48 85 c0             	test   %rax,%rax
    e32a:	0f 84 89 03 00 00    	je     e6b9 <code_add_into+0x8e9>
    e330:	66 0f 6f 05 08 2d 00 	movdqa 0x2d08(%rip),%xmm0        # 11040 <no_value+0x7c0>
    e337:	00 
    e338:	0f 11 00             	movups %xmm0,(%rax)
    e33b:	f0 48 83 05 e4 ed 00 	lock addq $0x1,0xede4(%rip)        # 1d128 <live_blocks>
    e342:	00 01 
    e344:	4c 8d 68 10          	lea    0x10(%rax),%r13
    e348:	48 89 d8             	mov    %rbx,%rax
    e34b:	48 c1 e0 04          	shl    $0x4,%rax
    e34f:	48 8d 44 01 20       	lea    0x20(%rcx,%rax,1),%rax
    e354:	48 89 44 24 20       	mov    %rax,0x20(%rsp)
    e359:	48 8d 1c d8          	lea    (%rax,%rbx,8),%rbx
    e35d:	49 8b 04 24          	mov    (%r12),%rax
    e361:	48 85 c0             	test   %rax,%rax
    e364:	7e 71                	jle    e3d7 <code_add_into+0x607>
    e366:	31 ed                	xor    %ebp,%ebp
    e368:	49 8b 44 24 08       	mov    0x8(%r12),%rax
    e36d:	48 89 4c 24 18       	mov    %rcx,0x18(%rsp)
    e372:	48 8b 34 e8          	mov    (%rax,%rbp,8),%rsi
    e376:	48 89 f7             	mov    %rsi,%rdi
    e379:	48 89 74 24 08       	mov    %rsi,0x8(%rsp)
    e37e:	e8 5d 3d ff ff       	call   20e0 <strlen@plt>
    e383:	48 8b 74 24 08       	mov    0x8(%rsp),%rsi
    e388:	48 89 df             	mov    %rbx,%rdi
    e38b:	48 8d 50 05          	lea    0x5(%rax),%rdx
    e38f:	48 83 ee 04          	sub    $0x4,%rsi
    e393:	48 89 54 24 08       	mov    %rdx,0x8(%rsp)
    e398:	e8 03 3e ff ff       	call   21a0 <memcpy@plt>
    e39d:	48 8b 4c 24 20       	mov    0x20(%rsp),%rcx
    e3a2:	48 8d 43 04          	lea    0x4(%rbx),%rax
    e3a6:	48 8b 54 24 08       	mov    0x8(%rsp),%rdx
    e3ab:	48 89 04 e9          	mov    %rax,(%rcx,%rbp,8)
    e3af:	48 89 e8             	mov    %rbp,%rax
    e3b2:	48 8b 4c 24 18       	mov    0x18(%rsp),%rcx
    e3b7:	48 83 c5 01          	add    $0x1,%rbp
    e3bb:	48 c1 e0 04          	shl    $0x4,%rax
    e3bf:	48 01 d3             	add    %rdx,%rbx
    e3c2:	f3 41 0f 6f 44 04 10 	movdqu 0x10(%r12,%rax,1),%xmm0
    e3c9:	0f 11 44 01 20       	movups %xmm0,0x20(%rcx,%rax,1)
    e3ce:	49 8b 04 24          	mov    (%r12),%rax
    e3d2:	48 39 c5             	cmp    %rax,%rbp
    e3d5:	7c 91                	jl     e368 <code_add_into+0x598>
    e3d7:	48 89 41 10          	mov    %rax,0x10(%rcx)
    e3db:	48 8b 44 24 20       	mov    0x20(%rsp),%rax
    e3e0:	48 8b 7c 24 28       	mov    0x28(%rsp),%rdi
    e3e5:	48 89 41 18          	mov    %rax,0x18(%rcx)
    e3e9:	e8 62 3c ff ff       	call   2050 <free@plt>
    e3ee:	f0 48 83 2d 31 ed 00 	lock subq $0x1,0xed31(%rip)        # 1d128 <live_blocks>
    e3f5:	00 01 
    e3f7:	4d 89 6f 08          	mov    %r13,0x8(%r15)
    e3fb:	41 8b 07             	mov    (%r15),%eax
    e3fe:	83 e8 04             	sub    $0x4,%eax
    e401:	83 f8 01             	cmp    $0x1,%eax
    e404:	0f 87 1d 02 00 00    	ja     e627 <code_add_into+0x857>
    e40a:	49 8b 47 08          	mov    0x8(%r15),%rax
    e40e:	45 31 ed             	xor    %r13d,%r13d
    e411:	48 85 c0             	test   %rax,%rax
    e414:	74 07                	je     e41d <code_add_into+0x64d>
    e416:	4c 8b 28             	mov    (%rax),%r13
    e419:	48 8b 40 08          	mov    0x8(%rax),%rax
    e41d:	41 8b 0e             	mov    (%r14),%ecx
    e420:	8d 51 fc             	lea    -0x4(%rcx),%edx
    e423:	83 fa 01             	cmp    $0x1,%edx
    e426:	0f 87 e3 01 00 00    	ja     e60f <code_add_into+0x83f>
    e42c:	4a 8b 5c e8 f8       	mov    -0x8(%rax,%r13,8),%rbx
    e431:	48 89 df             	mov    %rbx,%rdi
    e434:	e8 a7 3c ff ff       	call   20e0 <strlen@plt>
    e439:	48 8d 44 03 01       	lea    0x1(%rbx,%rax,1),%rax
    e43e:	48 89 44 24 08       	mov    %rax,0x8(%rsp)
    e443:	31 d2                	xor    %edx,%edx
    e445:	eb 6c                	jmp    e4b3 <code_add_into+0x6e3>
    e447:	66 0f 1f 84 00 00 00 	nopw   0x0(%rax,%rax,1)
    e44e:	00 00 
    e450:	41 8b 0f             	mov    (%r15),%ecx
    e453:	48 c1 e0 04          	shl    $0x4,%rax
    e457:	8d 79 fc             	lea    -0x4(%rcx),%edi
    e45a:	83 ff 01             	cmp    $0x1,%edi
    e45d:	0f 87 30 01 00 00    	ja     e593 <code_add_into+0x7c3>
    e463:	49 8b 7f 08          	mov    0x8(%r15),%rdi
    e467:	48 85 ff             	test   %rdi,%rdi
    e46a:	0f 84 23 01 00 00    	je     e593 <code_add_into+0x7c3>
    e470:	4c 8d 64 07 10       	lea    0x10(%rdi,%rax,1),%r12
    e475:	45 85 d2             	test   %r10d,%r10d
    e478:	74 16                	je     e490 <code_add_into+0x6c0>
    e47a:	8b 03                	mov    (%rbx),%eax
    e47c:	83 f8 01             	cmp    $0x1,%eax
    e47f:	0f 85 1d 01 00 00    	jne    e5a2 <code_add_into+0x7d2>
    e485:	48 8b 43 08          	mov    0x8(%rbx),%rax
    e489:	48 83 40 f0 01       	addq   $0x1,-0x10(%rax)
    e48e:	66 90                	xchg   %ax,%ax
    e490:	4c 89 e7             	mov    %r12,%rdi
    e493:	e8 08 64 ff ff       	call   48a0 <code_release>
    e498:	f3 0f 6f 03          	movdqu (%rbx),%xmm0
    e49c:	41 0f 11 04 24       	movups %xmm0,(%r12)
    e4a1:	41 8b 06             	mov    (%r14),%eax
    e4a4:	48 89 ea             	mov    %rbp,%rdx
    e4a7:	83 e8 04             	sub    $0x4,%eax
    e4aa:	83 f8 01             	cmp    $0x1,%eax
    e4ad:	0f 87 1d 01 00 00    	ja     e5d0 <code_add_into+0x800>
    e4b3:	49 8b 76 08          	mov    0x8(%r14),%rsi
    e4b7:	48 85 f6             	test   %rsi,%rsi
    e4ba:	0f 84 10 01 00 00    	je     e5d0 <code_add_into+0x800>
    e4c0:	48 3b 16             	cmp    (%rsi),%rdx
    e4c3:	0f 8d 07 01 00 00    	jge    e5d0 <code_add_into+0x800>
    e4c9:	48 8d 6a 01          	lea    0x1(%rdx),%rbp
    e4cd:	48 89 e8             	mov    %rbp,%rax
    e4d0:	48 c1 e0 04          	shl    $0x4,%rax
    e4d4:	48 8d 1c 06          	lea    (%rsi,%rax,1),%rbx
    e4d8:	48 8b 04 24          	mov    (%rsp),%rax
    e4dc:	44 8b 53 04          	mov    0x4(%rbx),%r10d
    e4e0:	48 8b 04 d0          	mov    (%rax,%rdx,8),%rax
    e4e4:	48 85 c0             	test   %rax,%rax
    e4e7:	0f 89 63 ff ff ff    	jns    e450 <code_add_into+0x680>
    e4ed:	45 85 d2             	test   %r10d,%r10d
    e4f0:	74 14                	je     e506 <code_add_into+0x736>
    e4f2:	8b 03                	mov    (%rbx),%eax
    e4f4:	83 f8 01             	cmp    $0x1,%eax
    e4f7:	0f 85 c8 00 00 00    	jne    e5c5 <code_add_into+0x7f5>
    e4fd:	48 8b 43 08          	mov    0x8(%rbx),%rax
    e501:	48 83 40 f0 01       	addq   $0x1,-0x10(%rax)
    e506:	48 8b 46 08          	mov    0x8(%rsi),%rax
    e50a:	48 8b 34 d0          	mov    (%rax,%rdx,8),%rsi
    e50e:	49 8b 47 08          	mov    0x8(%r15),%rax
    e512:	48 8b 40 08          	mov    0x8(%rax),%rax
    e516:	48 89 f7             	mov    %rsi,%rdi
    e519:	48 89 74 24 18       	mov    %rsi,0x18(%rsp)
    e51e:	4e 8d 14 e8          	lea    (%rax,%r13,8),%r10
    e522:	4c 89 54 24 20       	mov    %r10,0x20(%rsp)
    e527:	e8 b4 3b ff ff       	call   20e0 <strlen@plt>
    e52c:	48 8b 74 24 18       	mov    0x18(%rsp),%rsi
    e531:	48 8b 7c 24 08       	mov    0x8(%rsp),%rdi
    e536:	4c 8d 60 05          	lea    0x5(%rax),%r12
    e53a:	4c 89 e2             	mov    %r12,%rdx
    e53d:	48 83 ee 04          	sub    $0x4,%rsi
    e541:	e8 5a 3c ff ff       	call   21a0 <memcpy@plt>
    e546:	48 8b 4c 24 08       	mov    0x8(%rsp),%rcx
    e54b:	4c 8b 54 24 20       	mov    0x20(%rsp),%r10
    e550:	48 8d 41 04          	lea    0x4(%rcx),%rax
    e554:	4c 01 e1             	add    %r12,%rcx
    e557:	49 89 02             	mov    %rax,(%r10)
    e55a:	41 8b 07             	mov    (%r15),%eax
    e55d:	48 89 4c 24 08       	mov    %rcx,0x8(%rsp)
    e562:	8d 50 fc             	lea    -0x4(%rax),%edx
    e565:	31 c0                	xor    %eax,%eax
    e567:	83 fa 01             	cmp    $0x1,%edx
    e56a:	77 0f                	ja     e57b <code_add_into+0x7ab>
    e56c:	49 8b 47 08          	mov    0x8(%r15),%rax
    e570:	48 8d 50 10          	lea    0x10(%rax),%rdx
    e574:	48 85 c0             	test   %rax,%rax
    e577:	48 0f 45 c2          	cmovne %rdx,%rax
    e57b:	4c 89 ea             	mov    %r13,%rdx
    e57e:	f3 0f 6f 03          	movdqu (%rbx),%xmm0
    e582:	49 83 c5 01          	add    $0x1,%r13
    e586:	48 c1 e2 04          	shl    $0x4,%rdx
    e58a:	0f 11 04 10          	movups %xmm0,(%rax,%rdx,1)
    e58e:	e9 0e ff ff ff       	jmp    e4a1 <code_add_into+0x6d1>
    e593:	49 89 c4             	mov    %rax,%r12
    e596:	45 85 d2             	test   %r10d,%r10d
    e599:	74 1c                	je     e5b7 <code_add_into+0x7e7>
    e59b:	8b 03                	mov    (%rbx),%eax
    e59d:	83 f8 01             	cmp    $0x1,%eax
    e5a0:	74 0c                	je     e5ae <code_add_into+0x7de>
    e5a2:	83 e8 04             	sub    $0x4,%eax
    e5a5:	83 f8 01             	cmp    $0x1,%eax
    e5a8:	0f 87 7c 3f ff ff    	ja     252a <code_add_into.cold>
    e5ae:	48 8b 43 08          	mov    0x8(%rbx),%rax
    e5b2:	48 83 40 f0 01       	addq   $0x1,-0x10(%rax)
    e5b7:	4d 85 e4             	test   %r12,%r12
    e5ba:	0f 85 d0 fe ff ff    	jne    e490 <code_add_into+0x6c0>
    e5c0:	e9 41 ff ff ff       	jmp    e506 <code_add_into+0x736>
    e5c5:	45 31 e4             	xor    %r12d,%r12d
    e5c8:	eb d8                	jmp    e5a2 <code_add_into+0x7d2>
    e5ca:	66 0f 1f 44 00 00    	nopw   0x0(%rax,%rax,1)
    e5d0:	48 83 7c 24 10 00    	cmpq   $0x0,0x10(%rsp)
    e5d6:	75 37                	jne    e60f <code_add_into+0x83f>
    e5d8:	49 8b 47 08          	mov    0x8(%r15),%rax
    e5dc:	4c 89 6c 24 10       	mov    %r13,0x10(%rsp)
    e5e1:	48 89 44 24 08       	mov    %rax,0x8(%rsp)
    e5e6:	48 8b 44 24 08       	mov    0x8(%rsp),%rax
    e5eb:	48 8b 4c 24 10       	mov    0x10(%rsp),%rcx
    e5f0:	48 8b 3c 24          	mov    (%rsp),%rdi
    e5f4:	48 89 08             	mov    %rcx,(%rax)
    e5f7:	48 8d 44 24 30       	lea    0x30(%rsp),%rax
    e5fc:	48 39 c7             	cmp    %rax,%rdi
    e5ff:	0f 84 ba f8 ff ff    	je     debf <code_add_into+0xef>
    e605:	e8 46 3a ff ff       	call   2050 <free@plt>
    e60a:	e9 b0 f8 ff ff       	jmp    debf <code_add_into+0xef>
    e60f:	49 8b 47 08          	mov    0x8(%r15),%rax
    e613:	4c 89 6c 24 10       	mov    %r13,0x10(%rsp)
    e618:	48 89 44 24 08       	mov    %rax,0x8(%rsp)
    e61d:	48 c7 40 f8 00 00 00 	movq   $0x0,-0x8(%rax)
    e624:	00 
    e625:	eb bf                	jmp    e5e6 <code_add_into+0x816>
    e627:	45 31 ed             	xor    %r13d,%r13d
    e62a:	31 c0                	xor    %eax,%eax
    e62c:	e9 ec fd ff ff       	jmp    e41d <code_add_into+0x64d>
    e631:	48 83 7c 24 10 00    	cmpq   $0x0,0x10(%rsp)
    e637:	0f 85 36 fc ff ff    	jne    e273 <code_add_into+0x4a3>
    e63d:	41 8b 0f             	mov    (%r15),%ecx
    e640:	41 8b 06             	mov    (%r14),%eax
    e643:	8d 51 fc             	lea    -0x4(%rcx),%edx
    e646:	83 e8 04             	sub    $0x4,%eax
    e649:	83 fa 01             	cmp    $0x1,%edx
    e64c:	77 27                	ja     e675 <code_add_into+0x8a5>
    e64e:	49 8b 4f 08          	mov    0x8(%r15),%rcx
    e652:	48 89 4c 24 08       	mov    %rcx,0x8(%rsp)
    e657:	48 85 c9             	test   %rcx,%rcx
    e65a:	74 37                	je     e693 <code_add_into+0x8c3>
    e65c:	48 8b 4c 24 08       	mov    0x8(%rsp),%rcx
    e661:	4c 8b 29             	mov    (%rcx),%r13
    e664:	83 f8 01             	cmp    $0x1,%eax
    e667:	77 20                	ja     e689 <code_add_into+0x8b9>
    e669:	31 c0                	xor    %eax,%eax
    e66b:	48 89 44 24 08       	mov    %rax,0x8(%rsp)
    e670:	e9 ce fd ff ff       	jmp    e443 <code_add_into+0x673>
    e675:	83 f8 01             	cmp    $0x1,%eax
    e678:	77 2a                	ja     e6a4 <code_add_into+0x8d4>
    e67a:	31 d2                	xor    %edx,%edx
    e67c:	45 31 ed             	xor    %r13d,%r13d
    e67f:	48 89 54 24 08       	mov    %rdx,0x8(%rsp)
    e684:	e9 ba fd ff ff       	jmp    e443 <code_add_into+0x673>
    e689:	4c 89 6c 24 10       	mov    %r13,0x10(%rsp)
    e68e:	e9 53 ff ff ff       	jmp    e5e6 <code_add_into+0x816>
    e693:	45 31 ed             	xor    %r13d,%r13d
    e696:	83 f8 01             	cmp    $0x1,%eax
    e699:	0f 86 a4 fd ff ff    	jbe    e443 <code_add_into+0x673>
    e69f:	e9 42 ff ff ff       	jmp    e5e6 <code_add_into+0x816>
    e6a4:	49 8b 47 08          	mov    0x8(%r15),%rax
    e6a8:	48 89 44 24 08       	mov    %rax,0x8(%rsp)
    e6ad:	e9 34 ff ff ff       	jmp    e5e6 <code_add_into+0x816>
    e6b2:	31 db                	xor    %ebx,%ebx
    e6b4:	e9 00 fb ff ff       	jmp    e1b9 <code_add_into+0x3e9>
    e6b9:	48 8d 3d c9 1c 00 00 	lea    0x1cc9(%rip),%rdi        # 10389 <_fini+0x529>
    e6c0:	e8 3b 53 ff ff       	call   3a00 <code_runtime_error>

Disassembly of section .fini:
